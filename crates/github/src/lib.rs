//! GitHub provider (reference implementation, plans/0005): wraps the
//! REST `Client` — auth resolution, sha-keyed disk cache, ETag
//! revalidation all live inside it (PLAN.md §7/§8).

use rootle_provider::{
    BlameRange, Capabilities, CodeMatch, CommitDetail, CommitFile, FileStatus, GitRef, LogEntry,
    Provider, ProviderResult, RepoId, RepoInfo, RepoRefs, SearchItem, Sha, TreeNode, TreeResult,
};
pub mod cache;
pub mod client;
pub mod types;

use crate::client::GitHubClient;

pub struct GitHubProvider {
    client: GitHubClient,
}

impl GitHubProvider {
    pub fn new(max_mb: u64) -> Self {
        // Self-hardening: orphan sweep + LRU eviction of the content
        // store, off-thread — the TUI never knows this exists.
        let max_bytes = max_mb * 1024 * 1024;
        std::thread::spawn(move || crate::cache::harden(max_bytes));
        GitHubProvider {
            client: GitHubClient::new(),
        }
    }

    /// Token-less provider (tests, offline defaults).
    /// Token-less, no hardening (tests and offline defaults).
    pub fn anonymous() -> Self {
        GitHubProvider {
            client: GitHubClient::anonymous(),
        }
    }
}

fn split_repo(repo: &RepoId) -> Result<(&str, &str), String> {
    repo.as_str()
        .split_once('/')
        .ok_or_else(|| format!("bad repo id: {repo} (expected owner/name)"))
}

impl From<&crate::types::TreeEntry> for TreeNode {
    fn from(e: &crate::types::TreeEntry) -> Self {
        TreeNode {
            path: e.path.clone(),
            is_dir: e.kind == "tree",
            sha: e.sha.clone(),
            size: e.size,
        }
    }
}

impl Provider for GitHubProvider {
    fn name(&self) -> &str {
        "github"
    }

    fn icon(&self) -> Option<String> {
        Some("github".into())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            orgs: true,
            code_search: true,
            file_search: true,
            // v1.5: branches/tags, log, blame. v1.6: commit detail.
            refs: true,
            log: true,
            blame: true,
            commit: true,
        }
    }

    fn default_orgs(&self) -> Vec<String> {
        ["ratatui", "tokio-rs", "helix-editor"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn search(&self, query: &str) -> ProviderResult<Vec<SearchItem>> {
        self.client.search(query)
    }

    fn org_repos(&self, org: &str) -> ProviderResult<Vec<RepoInfo>> {
        self.client.org_repos(org)
    }

    fn fetch_tree(&self, repo: &RepoId, ref_: Option<&GitRef>) -> ProviderResult<TreeResult> {
        let (owner, name) = split_repo(repo)?;
        let (tree, truncated, branch) =
            self.client
                .fetch_tree(owner, name, ref_.map(|r| r.as_str()))?;
        Ok(TreeResult {
            entries: tree.tree.iter().map(Into::into).collect(),
            truncated,
            branch,
        })
    }

    fn fetch_blob(&self, repo: &RepoId, sha: &Sha) -> ProviderResult<Vec<u8>> {
        let (owner, name) = split_repo(repo)?;
        self.client.fetch_blob(owner, name, sha.as_str())
    }

    /// v1.5 (plans/0016 M1): branches + tags.
    fn refs(&self, repo: &RepoId) -> ProviderResult<RepoRefs> {
        let (owner, name) = split_repo(repo)?;
        self.client.refs(owner, name)
    }

    /// v1.5: commit log, newest first.
    fn log(
        &self,
        repo: &RepoId,
        path: Option<&str>,
        ref_: Option<&GitRef>,
        limit: Option<usize>,
    ) -> ProviderResult<(Vec<LogEntry>, bool)> {
        let (owner, name) = split_repo(repo)?;
        self.client
            .log(owner, name, path, ref_.map(|r| r.as_str()), limit)
    }

    /// v1.5: open-at-commit.
    fn blob_at(
        &self,
        repo: &RepoId,
        path: &str,
        ref_: Option<&GitRef>,
    ) -> ProviderResult<(Vec<u8>, Sha)> {
        let (owner, name) = split_repo(repo)?;
        self.client
            .blob_at(owner, name, path, ref_.map(|r| r.as_str()))
            .map(|(bytes, sha)| (bytes, Sha::from(sha)))
    }

    /// v1.5: blame via GraphQL.
    fn blame(
        &self,
        repo: &RepoId,
        path: &str,
        ref_: Option<&GitRef>,
    ) -> ProviderResult<Vec<BlameRange>> {
        let (owner, name) = split_repo(repo)?;
        self.client
            .blame(owner, name, path, ref_.map(|r| r.as_str()))
    }

    /// v1.6 (plans/0028): one commit's detail.
    fn commit(&self, repo: &RepoId, sha: &Sha) -> ProviderResult<CommitDetail> {
        let (owner, name) = split_repo(repo)?;
        Ok(self.client.commit(owner, name, sha.as_str())?.into())
    }

    fn clone_url(&self, repo: &RepoId) -> ProviderResult<String> {
        split_repo(repo)?;
        Ok(format!("https://github.com/{repo}.git"))
    }

    fn web_url(
        &self,
        repo: &RepoId,
        path: &str,
        branch: Option<&GitRef>,
        line: Option<u32>,
        end: Option<u32>,
        is_file: bool,
    ) -> ProviderResult<String> {
        split_repo(repo)?;
        if path.is_empty() {
            return Ok(format!("https://github.com/{repo}"));
        }
        // Blob vs tree grammar; the branch is cheap to resolve — the
        // tree is disk-cached whenever the repo has been browsed.
        let branch = match branch {
            None => self.fetch_tree(repo, None).map(|t| t.branch)?,
            Some(b) => b.as_str().to_string(),
        };
        let kind = if is_file { "blob" } else { "tree" };
        // Range anchors (v1.5): `#L3-L7` when a selection's end rides
        // along (GitHub's fragment grammar).
        let fragment = match (is_file, line, end) {
            (true, Some(line), Some(end)) if end > line => format!("#L{line}-L{end}"),
            (true, Some(line), _) => format!("#L{line}"),
            _ => String::new(),
        };
        Ok(format!(
            "https://github.com/{repo}/{kind}/{branch}/{path}{fragment}"
        ))
    }

    fn org_url(&self, org: &str) -> ProviderResult<String> {
        Ok(format!("https://github.com/{org}"))
    }

    fn search_code(&self, q: &str) -> ProviderResult<rootle_provider::SearchCodeResult> {
        let (items, truncated) = self.client.search_code(q)?;
        Ok(rootle_provider::SearchCodeResult {
            hits: items.iter().map(CodeMatch::from).collect(),
            truncated,
            index_as_of: None,
        })
    }

    fn source_tarball(&self, repo: &RepoId) -> ProviderResult<Vec<u8>> {
        split_repo(repo)?;
        self.client.source_tarball(repo.as_str())
    }

    /// v1.3 progressive (plans/0011): stream `search/code` pages as
    /// they arrive — the first 100 render while later pages fetch.
    /// Budget: 3 pages × 100 (rate-conscious); GitHub caps code search
    /// at 1000 anyway, and `truncated` says whether more exists.
    fn search_code_progressive(
        &self,
        q: &str,
        on_hits: &(dyn Fn(&[CodeMatch]) + Send + Sync),
    ) -> ProviderResult<rootle_provider::SearchCodeResult> {
        const PAGES: u32 = 3;
        const PER_PAGE: usize = 100;
        let mut fetched = 0usize;
        let mut total = 0u64;
        for page in 1..=PAGES {
            let (items, page_total) = self.client.search_code_page(q, page)?;
            total = page_total;
            fetched += items.len();
            let empty = items.is_empty();
            let matches: Vec<CodeMatch> = items.iter().map(CodeMatch::from).collect();
            on_hits(&matches);
            if empty || items.len() < PER_PAGE {
                break;
            }
        }
        Ok(rootle_provider::SearchCodeResult {
            hits: Vec::new(),
            truncated: (total as usize) > fetched,
            // GitHub's index freshness isn't exposed — no badge.
            index_as_of: None,
        })
    }
}

impl From<&crate::types::CodeItem> for CodeMatch {
    fn from(item: &crate::types::CodeItem) -> Self {
        CodeMatch {
            repo: item.repository.full_name.clone(),
            path: item.path.clone(),
            sha: item.sha.clone(),
            branch: item
                .repository
                .default_branch
                .clone()
                .unwrap_or_else(|| "main".into()),
            matches: item
                .text_matches
                .iter()
                .flat_map(|tm| tm.matches.iter().map(|m| m.text.clone()))
                .collect(),
            located: true,
            // GitHub text-match fragments carry no absolute line
            // numbers — locating fills them.
            line: None,
        }
    }
}

impl From<crate::types::CommitResponse> for CommitDetail {
    fn from(response: crate::types::CommitResponse) -> Self {
        let author = response.commit.author;
        let (author, date) = author
            .map(|author| {
                (
                    author.name.unwrap_or_default(),
                    author.date.unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        CommitDetail {
            sha: response.sha.into(),
            author,
            date,
            message: response.commit.message,
            parents: response
                .parents
                .into_iter()
                .map(|parent| parent.sha.into())
                .collect(),
            files: response
                .files
                .into_iter()
                .map(|file| CommitFile {
                    path: file.filename.into(),
                    status: file_status(file.status.as_deref()),
                    additions: file.additions,
                    deletions: file.deletions,
                    patch: file.patch,
                    previous_path: file.previous_filename.map(Into::into),
                    binary: false,
                })
                .collect(),
            truncated: response.truncated,
            web_url: response.html_url,
        }
    }
}

/// Wire `status` (lowercase) → `FileStatus`. GitHub also emits
/// "changed"/"unchanged"; anything the seam doesn't name degrades to
/// `Modified` (the v1.6 reader-tolerance rule).
fn file_status(s: Option<&str>) -> FileStatus {
    match s {
        Some("added") => FileStatus::Added,
        Some("removed") => FileStatus::Removed,
        Some("renamed") => FileStatus::Renamed,
        _ => FileStatus::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_url_grammar() {
        let p = GitHubProvider::anonymous();
        // Repo root.
        assert_eq!(
            p.web_url(
                &RepoId::from("ratatui/ratatui"),
                "",
                None,
                None,
                None,
                false
            )
            .unwrap(),
            "https://github.com/ratatui/ratatui"
        );
        // File with a line: blob + fragment (known branch, no I/O).
        assert_eq!(
            p.web_url(
                &RepoId::from("ratatui/ratatui"),
                "src/lib.rs",
                Some(&GitRef::from("main")),
                Some(42),
                None,
                true
            )
            .unwrap(),
            "https://github.com/ratatui/ratatui/blob/main/src/lib.rs#L42"
        );
        // A visual range anchors `#L3-L7` (v1.5).
        assert_eq!(
            p.web_url(
                &RepoId::from("ratatui/ratatui"),
                "src/lib.rs",
                Some(&GitRef::from("main")),
                Some(3),
                Some(7),
                true
            )
            .unwrap(),
            "https://github.com/ratatui/ratatui/blob/main/src/lib.rs#L3-L7"
        );
        // File without a line: blob, no fragment.
        assert_eq!(
            p.web_url(
                &RepoId::from("ratatui/ratatui"),
                "src/lib.rs",
                Some(&GitRef::from("main")),
                None,
                None,
                true
            )
            .unwrap(),
            "https://github.com/ratatui/ratatui/blob/main/src/lib.rs"
        );
        // Directory: tree.
        assert_eq!(
            p.web_url(
                &RepoId::from("ratatui/ratatui"),
                "src",
                Some(&GitRef::from("master")),
                None,
                None,
                false
            )
            .unwrap(),
            "https://github.com/ratatui/ratatui/tree/master/src"
        );
        assert_eq!(p.org_url("ratatui").unwrap(), "https://github.com/ratatui");
    }

    #[test]
    fn clone_url_grammar() {
        let p = GitHubProvider::anonymous();
        assert_eq!(
            p.clone_url(&RepoId::from("ratatui/ratatui")).unwrap(),
            "https://github.com/ratatui/ratatui.git"
        );
        assert!(p.clone_url(&RepoId::from("no-slash")).is_err());
    }

    /// The v1.6 commit mapping: statuses degrade, renames carry
    /// their old path, absent counts stay `None`.
    #[test]
    fn commit_detail_maps_from_wire() {
        let json = r#"{
            "sha": "6dcb09b5",
            "commit": {
                "message": "Fix the race\n\nBody line.",
                "author": {"name": "octocat", "date": "2026-09-06T10:00:00Z"}
            },
            "parents": [{"sha": "aaa111"}, {"sha": "bbb222"}],
            "files": [
                {"filename": "src/lib.rs", "status": "modified", "additions": 2,
                 "deletions": 1, "patch": "@@ -1 +1,2 @@\n-old\n+new\n+line"},
                {"filename": "src/moved.rs", "status": "renamed",
                 "previous_filename": "src/orig.rs", "additions": 1, "deletions": 1},
                {"filename": "img/logo.png", "status": "changed", "additions": 3, "deletions": 0}
            ]
        }"#;
        let wire: crate::types::CommitResponse = serde_json::from_str(json).unwrap();
        let d = CommitDetail::from(wire);
        assert_eq!(d.files[0].status, FileStatus::Modified);
        assert_eq!(d.files[1].status, FileStatus::Renamed);
        assert_eq!(
            d.files[1].previous_path.as_ref().map(|path| path.as_str()),
            Some("src/orig.rs")
        );
        // "changed" is not one of the seam's four → modified.
        assert_eq!(d.files[2].status, FileStatus::Modified);
        assert_eq!(d.files[2].patch, None);
        assert_eq!(
            d.line_stats(),
            rootle_provider::CommitStatistics {
                additions: Some(6),
                deletions: Some(2)
            }
        );
    }
}
