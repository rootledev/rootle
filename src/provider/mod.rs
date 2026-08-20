//! Provider seam (plans/0005): the TUI talks to source-control backends
//! through this trait — never to a concrete API. `github` is the
//! in-tree reference implementation; external providers are child
//! processes speaking NDJSON-RPC over stdio (`stdio.rs`), so any
//! internal system can be wrapped with a small script.
//!
//! Contract rules that matter:
//! - Repos are opaque "group/project" strings; the UI never parses them.
//! - `sha` is an opaque *content id*: it MUST change when content
//!   changes (the cache design is content-keyed and immutable).
//! - URL building (yank) and cloning use provider-supplied fields —
//!   no GitHub URL grammar outside the GitHub impl.

pub mod github;
pub mod stdio;

use std::sync::Arc;

/// What a provider supports; the UI degrades on `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub orgs: bool,
    pub code_search: bool,
}

/// Repo/org search result for the launch popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchItem {
    /// "group/project"
    Repo(String),
    /// org/group name
    Org(String),
}

/// UI-facing tree node (path relative to repo root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub path: String,
    pub is_dir: bool,
    pub sha: String,
    pub size: Option<u64>,
}

/// A repo's recursive tree plus routing metadata.
#[derive(Debug, Clone)]
pub struct TreeResult {
    pub entries: Vec<TreeNode>,
    pub truncated: bool,
    pub branch: String,
}

/// One code-search hit. `matches` are the matched substrings (the UI
/// locates them in the blob for real line numbers).
#[derive(Debug, Clone)]
pub struct CodeMatch {
    pub repo: String,
    pub path: String,
    pub sha: String,
    pub branch: String,
    pub matches: Vec<String>,
}

/// The backend contract. Blocking; calls run on worker threads.
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    /// Suggested orgs for a cold start (no state); GitHub ships its
    /// defaults, other providers return nothing.
    fn default_orgs(&self) -> Vec<String> {
        Vec::new()
    }

    /// Repo + org search for the launch popup (orgs first).
    fn search(&self, query: &str) -> Result<Vec<SearchItem>, String>;
    /// Repo names of an org/group.
    fn org_repos(&self, org: &str) -> Result<Vec<String>, String>;
    /// Full recursive tree of a repo's default branch.
    fn fetch_tree(&self, repo: &str) -> Result<TreeResult, String>;
    /// Blob bytes by content id.
    fn fetch_blob(&self, repo: &str, sha: &str) -> Result<Vec<u8>, String>;
    /// Code search; `q` is the full query string with qualifiers.
    fn search_code(&self, q: &str) -> Result<Vec<CodeMatch>, String>;

    /// URL `git clone` accepts for a repo (clone wizard, plans/0004).
    fn clone_url(&self, repo: &str) -> Result<String, String>;

    /// Browser URL for yank (␣ y): repo root, or a path/line inside
    /// it. `branch` may be empty (the provider resolves it).
    fn web_url(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        line: Option<u32>,
    ) -> Result<String, String>;

    /// Browser URL for an org/group page.
    fn org_url(&self, org: &str) -> Result<String, String>;
}

/// Build the configured provider. Invalid/unsupported config falls
/// back to GitHub (with a warning string for the status line) — a
/// provider misconfiguration must never block startup.
pub fn build(config: &crate::config::Config) -> (Arc<dyn Provider>, Option<String>) {
    match config.provider.kind.as_str() {
        "github" => (
            Arc::new(github::GitHubProvider::new(config.cache.max_mb)),
            None,
        ),
        "stdio" => match stdio::StdioProvider::spawn(&config.provider.command) {
            Ok(p) => (Arc::new(p), None),
            Err(e) => (
                Arc::new(github::GitHubProvider::new(config.cache.max_mb)),
                Some(format!("provider stdio failed ({e}); fell back to github")),
            ),
        },
        other => (
            Arc::new(github::GitHubProvider::new(config.cache.max_mb)),
            Some(format!("unknown provider kind {other:?}; using github")),
        ),
    }
}

/// Offline provider for tests: every call errors, nothing spawns.
pub fn offline() -> Arc<dyn Provider> {
    struct Offline;
    impl Provider for Offline {
        fn name(&self) -> &str {
            "offline"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                orgs: false,
                code_search: false,
            }
        }
        fn search(&self, _: &str) -> Result<Vec<SearchItem>, String> {
            Err("offline".into())
        }
        fn org_repos(&self, _: &str) -> Result<Vec<String>, String> {
            Err("offline".into())
        }
        fn fetch_tree(&self, _: &str) -> Result<TreeResult, String> {
            Err("offline".into())
        }
        fn fetch_blob(&self, _: &str, _: &str) -> Result<Vec<u8>, String> {
            Err("offline".into())
        }
        fn search_code(&self, _: &str) -> Result<Vec<CodeMatch>, String> {
            Err("offline".into())
        }
        fn clone_url(&self, _: &str) -> Result<String, String> {
            Err("offline".into())
        }
        fn web_url(&self, _: &str, _: &str, _: &str, _: Option<u32>) -> Result<String, String> {
            Err("offline".into())
        }
        fn org_url(&self, _: &str) -> Result<String, String> {
            Err("offline".into())
        }
    }
    Arc::new(Offline)
}
