//! Thin blocking client. One instance, shared across worker threads
//! via Arc (reqwest::blocking::Client is Sync).

use super::types::{OrgRepoItem, SearchItem, SearchReposResponse, SearchUsersResponse};
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
        let token = std::env::var("GHX_TOKEN")
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
            .user_agent("ghx")
            .timeout(std::time::Duration::from_secs(15))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client build");
        Client { http, token }
    }

    pub fn is_anonymous(&self) -> bool {
        self.token.is_none()
    }

    fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let mut req = self.http.get(url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        resp.json::<T>().map_err(|e| e.to_string())
    }

    /// Repo search + org search, merged: orgs first, then repos.
    pub fn search(&self, query: &str) -> Result<Vec<SearchItem>, String> {
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

    pub fn org_repos(&self, org: &str) -> Result<Vec<String>, String> {
        let repos: Vec<OrgRepoItem> =
            self.get(&format!("{API}/orgs/{org}/repos?per_page=100&sort=updated"))?;
        Ok(repos.into_iter().map(|r| r.name).collect())
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
