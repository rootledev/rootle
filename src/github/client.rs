//! Thin blocking client. One instance, shared across worker threads
//! via Arc (reqwest::blocking::Client is Sync).

use super::types::{OrgRepoItem, RepoMeta, SearchReposResponse, SearchUsersResponse, TreeResponse};
use crate::provider::{ErrorKind, ProviderError, ProviderResult, SearchItem};
use std::process::Command;

const API: &str = "https://api.github.com";

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::blocking::Client,
    token: Option<String>,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}
impl Client {
    pub fn new() -> Self {
        let token = std::env::var("ROOTLE_TOKEN")
            .ok()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .filter(|t| !t.is_empty())
            .or_else(gh_token);
        Self::build(token)
    }

    /// No env, no shell-out — for tests and offline defaults.
    pub fn anonymous() -> Self {
        Self::build(None)
    }

    fn build(token: Option<String>) -> Self {
        // Hard timeout on every request: workers run off-thread, so a
        // hung request can't block the UI — but without this the status
        // line would say "searching…" forever (PLAN.md §7).
        let http = reqwest::blocking::Client::builder()
            .user_agent("rootle")
            .timeout(std::time::Duration::from_secs(15))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client build");
        Client { http, token }
    }

    pub fn is_anonymous(&self) -> bool {
        self.token.is_none()
    }

    fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> ProviderResult<T> {
        let mut req = self.http.get(url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().map_err(classify_send)?;
        if !resp.status().is_success() {
            return Err(classify_status(resp));
        }
        resp.json::<T>()
            .map_err(|e| ProviderError::other(e.to_string()))
    }

    /// Repo search + org search, merged: orgs first, then repos.
    /// Returns provider-level items (the trait boundary type).
    pub fn search(&self, query: &str) -> ProviderResult<Vec<SearchItem>> {
        let q = urlencoding(query);
        let mut out = Vec::new();

        let orgs: SearchUsersResponse =
            self.get(&format!("{API}/search/users?q={q}+type:org&per_page=5"))?;
        out.extend(orgs.items.into_iter().map(|u| SearchItem::Org(u.login)));

        let repos: SearchReposResponse =
            self.get(&format!("{API}/search/repositories?q={q}&per_page=20"))?;
        out.extend(
            repos
                .items
                .into_iter()
                .map(|r| SearchItem::Repo(r.full_name)),
        );

        Ok(out)
    }

    pub fn org_repos(&self, org: &str) -> ProviderResult<Vec<String>> {
        let repos: Vec<OrgRepoItem> =
            self.get(&format!("{API}/orgs/{org}/repos?per_page=100&sort=updated"))?;
        Ok(repos.into_iter().map(|r| r.name).collect())
    }

    /// Code search (plans/0002 §4). Requires auth — anonymous clients
    /// get a clear error. `q` is the full query string including
    /// qualifiers (`repo:`, `org:`, `extension:`, `path:`); text-match
    /// fragments are requested for previews.
    pub fn search_code(&self, q: &str) -> ProviderResult<(Vec<super::types::CodeItem>, bool)> {
        if self.is_anonymous() {
            return Err(ProviderError::new(
                ErrorKind::Auth,
                "code search needs a token — set ROOTLE_TOKEN or log in with `gh`",
            ));
        }
        let url = format!("{API}/search/code?q={}&per_page=25", urlencoding(q));
        let resp: super::types::SearchCodeResponse =
            self.get_accept(&url, "application/vnd.github.text-match+json")?;
        // GitHub caps code search at 1000 results — that is the
        // provider's own truncation signal (plans/0008 §4), not the
        // per-page size (that's our client-side clip).
        Ok((resp.items, resp.total_count > 1000))
    }

    /// GET with an explicit Accept header (text-match fragments).
    fn get_accept<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        accept: &str,
    ) -> ProviderResult<T> {
        let mut req = self.http.get(url).header("Accept", accept);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().map_err(classify_send)?;
        if !resp.status().is_success() {
            return Err(classify_status(resp));
        }
        resp.json::<T>()
            .map_err(|e| ProviderError::other(e.to_string()))
    }

    /// Fetch a repo's full recursive tree with the sha-keyed cache
    /// (PLAN.md §8): revalidate the branch ref with If-None-Match
    /// (304 = free, tree unchanged), cache tree bodies by their sha.
    /// Also returns the default branch (URL building, yank).
    pub fn fetch_tree(
        &self,
        owner: &str,
        repo: &str,
    ) -> ProviderResult<(
        TreeResponse,
        /*truncated*/ bool,
        /*branch*/ String,
    )> {
        // Cache-first branch resolution: a repo we've opened before
        // costs zero extra calls here (no GET /repos/{o}/{r}).
        let cached_branch = super::cache::cached_branch(owner, repo);
        let branch = match &cached_branch {
            Some(b) => {
                crate::app::trace(&format!("tree branch cached {owner}/{repo} {b}"));
                b.clone()
            }
            None => {
                crate::app::trace(&format!("tree branch meta-fetch {owner}/{repo}"));
                let meta: RepoMeta = self.get(&format!("{API}/repos/{owner}/{repo}"))?;
                meta.default_branch
            }
        };
        match self.fetch_tree_on(owner, repo, &branch) {
            // The default branch was renamed since we cached it:
            // resolve fresh and try once more.
            Err(e) if cached_branch.is_some() && e.kind == ErrorKind::NotFound => {
                let meta: RepoMeta = self.get(&format!("{API}/repos/{owner}/{repo}"))?;
                self.fetch_tree_on(owner, repo, &meta.default_branch)
            }
            other => other,
        }
    }

    fn fetch_tree_on(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> ProviderResult<(
        TreeResponse,
        /*truncated*/ bool,
        /*branch*/ String,
    )> {
        let cached_ref = super::cache::read_ref(owner, repo, branch);
        let url = format!("{API}/repos/{owner}/{repo}/git/trees/{branch}?recursive=1");

        let etag = cached_ref.as_ref().and_then(|r| r.etag.clone());
        match self.get_conditional::<TreeResponse>(&url, etag.as_deref())? {
            Conditional::NotModified => {
                let sha = cached_ref.expect("304 without a cached ref").tree_sha;
                let tree = super::cache::read_tree(&sha).ok_or_else(|| {
                    ProviderError::other(format!("304 but tree {sha} missing from cache"))
                })?;
                Ok((tree.clone(), tree.truncated, branch.to_string()))
            }
            Conditional::Fresh { body, etag } => {
                super::cache::write_tree(&body).map_err(|e| ProviderError::other(e.to_string()))?;
                super::cache::write_ref(
                    owner,
                    repo,
                    branch,
                    &super::cache::RefCache {
                        tree_sha: body.sha.clone(),
                        etag,
                    },
                )
                .map_err(|e| ProviderError::other(e.to_string()))?;
                let truncated = body.truncated;
                Ok((body, truncated, branch.to_string()))
            }
        }
    }

    /// Fetch a blob by git sha, cache-first (blobs are immutable).
    /// Files over 1 MiB are rejected — too heavy for a preview pane.
    pub fn fetch_blob(&self, owner: &str, repo: &str, sha: &str) -> ProviderResult<Vec<u8>> {
        if let Some(bytes) = super::cache::read_blob(sha) {
            return Ok(bytes);
        }
        #[derive(serde::Deserialize)]
        struct BlobResponse {
            content: String,
            size: u64,
        }
        let url = format!("{API}/repos/{owner}/{repo}/git/blobs/{sha}");
        let blob: BlobResponse = self.get(&url)?;
        if blob.size > 1024 * 1024 {
            return Err(ProviderError::other(format!(
                "file too large to preview ({} bytes)",
                blob.size
            )));
        }
        use base64::Engine;
        let clean: String = blob
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(clean)
            .map_err(|e| ProviderError::other(e.to_string()))?;
        let _ = super::cache::write_blob(sha, &bytes);
        Ok(bytes)
    }

    fn get_conditional<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> ProviderResult<Conditional<T>> {
        let mut req = self.http.get(url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let resp = req.send().map_err(classify_send)?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(Conditional::NotModified);
        }
        if !status.is_success() {
            return Err(classify_status(resp));
        }
        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = resp
            .json::<T>()
            .map_err(|e| ProviderError::other(e.to_string()))?;
        Ok(Conditional::Fresh { body, etag })
    }
}

enum Conditional<T> {
    NotModified,
    Fresh { body: T, etag: Option<String> },
}

/// Classify a transport-level failure (plans/0008 §2).
fn classify_send(e: reqwest::Error) -> ProviderError {
    let kind = if e.is_timeout() {
        ErrorKind::Timeout
    } else {
        ErrorKind::Network
    };
    ProviderError::new(kind, e.to_string())
}

/// Classify a non-2xx reply into the error taxonomy: 401/403 → auth
/// (403 with an exhausted rate limit is throttling, not auth), 404 →
/// not_found, 429 → rate_limited (Retry-After rides along), 5xx →
/// provider, anything else → other.
fn classify_status(resp: reqwest::blocking::Response) -> ProviderError {
    let status = resp.status();
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(std::time::Duration::from_secs);
    let remaining_zero = resp
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "0");
    let kind = match status.as_u16() {
        401 => ErrorKind::Auth,
        403 if remaining_zero => ErrorKind::RateLimited,
        403 => ErrorKind::Auth,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::Provider,
        _ => ErrorKind::Other,
    };
    let error = ProviderError::new(kind, format!("HTTP {status}"));
    match retry_after {
        Some(d) => error.with_retry_after(d),
        None => error,
    }
}

fn gh_token() -> Option<String> {
    let out = Command::new("gh").args(["auth", "token"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Minimal percent-encoding for query strings (avoid a dependency for this).
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '/') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query_strings() {
        assert_eq!(urlencoding("ratatui tui"), "ratatui%20tui");
        assert_eq!(urlencoding("owner/repo"), "owner/repo");
    }
}
