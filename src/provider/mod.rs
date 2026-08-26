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
pub mod manager;
pub mod stdio;
pub mod ui;

use std::sync::Arc;

/// Structured provider error (plans/0008 §2): the protocol v1.1
/// `data.kind` taxonomy carried from the wire to the UI instead of a
/// bare string. Unknown or absent kinds degrade to `Other`, which
/// renders exactly like the old unstructured toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub message: String,
    /// `rate_limited`: the provider's advertised backoff, if any.
    pub retry_after: Option<std::time::Duration>,
}

/// The v1.1 `data.kind` open enum. Wire-unknown kinds map to `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Auth,
    RateLimited,
    NotFound,
    Network,
    Timeout,
    Provider,
    Other,
}

impl ProviderError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        ProviderError {
            kind,
            message: message.into(),
            retry_after: None,
        }
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Other, message)
    }

    pub fn with_retry_after(mut self, retry: std::time::Duration) -> Self {
        self.retry_after = Some(retry);
        self
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProviderError {}

impl From<String> for ProviderError {
    fn from(message: String) -> Self {
        Self::other(message)
    }
}

impl From<&str> for ProviderError {
    fn from(message: &str) -> Self {
        Self::other(message)
    }
}

pub type ProviderResult<T> = std::result::Result<T, ProviderError>;

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
    /// v1.1: provider knows its index is stale for this hit (the UI
    /// shows a stale chip until client-side locating self-heals).
    pub located: bool,
}

/// Code-search page plus the provider's own truncation signal
/// (plans/0008 §4): a backend that caps its result set says so;
/// absent on the wire means `false` (complete).
#[derive(Debug, Clone)]
pub struct SearchCodeResult {
    pub hits: Vec<CodeMatch>,
    pub truncated: bool,
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
    fn search(&self, query: &str) -> ProviderResult<Vec<SearchItem>>;
    /// Repo names of an org/group.
    fn org_repos(&self, org: &str) -> ProviderResult<Vec<String>>;
    /// Full recursive tree of a repo's default branch.
    fn fetch_tree(&self, repo: &str) -> ProviderResult<TreeResult>;
    /// Blob bytes by content id.
    fn fetch_blob(&self, repo: &str, sha: &str) -> ProviderResult<Vec<u8>>;
    /// Code search; `q` is the full query string with qualifiers.
    fn search_code(&self, q: &str) -> ProviderResult<SearchCodeResult>;

    /// Advisory cancellation (protocol v1.1): tells the backend the
    /// caller no longer needs the in-flight request. Best-effort —
    /// replies may still arrive and are always handled. Default: nothing
    /// to cancel (in-process providers drop work via generations).
    fn advise_cancel(&self) {}

    /// One-shot UI notice for the status line (plans/0008 §5) — e.g.
    /// a stdio child's successful restart. Drained once per route.
    /// Default: nothing to say.
    fn take_notice(&self) -> Option<String> {
        None
    }

    /// Cache usage the provider reported at initialize (bytes), when
    /// it participates in the advisory cache budget (protocol v1.2) —
    /// surfaced in :settings next to the provider row.
    fn cache_usage(&self) -> Option<u64> {
        None
    }

    /// URL `git clone` accepts for a repo (clone wizard, plans/0004).
    fn clone_url(&self, repo: &str) -> ProviderResult<String>;
    /// Browser URL for yank (␣ y): repo root, or a path inside it.
    /// `is_file` picks the grammar (GitHub: blob vs tree); `line`
    /// adds a fragment when Some. `branch` may be empty (the provider
    /// resolves it).
    fn web_url(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        line: Option<u32>,
        is_file: bool,
    ) -> ProviderResult<String>;

    /// Browser URL for an org/group page.
    fn org_url(&self, org: &str) -> ProviderResult<String>;
}

/// The provider's cache-subtree name from its argv: the binary's full
/// file stem (rootle-gitlab → rootle-gitlab) — matching the protocol
/// doc's `providers/<name>/` convention adapters document as their
/// default, so the handshake's cache_dir and the adapter's own
/// default are the same directory.
fn name_from_command(command: &[String]) -> String {
    command
        .first()
        .and_then(|c| std::path::Path::new(c).file_stem().and_then(|s| s.to_str()))
        .unwrap_or("provider")
        .to_string()
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
        "stdio" => {
            // Recognized values: unset/empty, "null" (discard), and
            // "inherit" (pass through). Anything else discards with a
            // warning — a typo shouldn't silently disable debugging.
            let stderr = config.provider.stderr.trim();
            let inherit = stderr == "inherit";
            let warn = if inherit || stderr.is_empty() || stderr == "null" {
                None
            } else {
                Some(format!(
                    "provider stderr {stderr:?} not recognized (use \"inherit\" or \"null\"); discarding child stderr"
                ))
            };
            // The user's cache budget and this provider's subtree
            // travel in every initialize (protocol v1.2, advisory) —
            // one [cache] max_mb knob governs every backend.
            let cache_bytes = config.cache.max_mb * 1024 * 1024;
            let cache_dir = dirs::cache_dir().map(|d| {
                d.join("rootle")
                    .join("providers")
                    .join(name_from_command(&config.provider.command))
            });
            match stdio::StdioProvider::spawn_with_cache(
                &config.provider.command,
                std::time::Duration::from_millis(config.provider.timeout_ms),
                inherit,
                cache_bytes,
                cache_dir,
            ) {
                Ok(p) => (Arc::new(p), warn),
                Err(e) => (
                    Arc::new(github::GitHubProvider::new(config.cache.max_mb)),
                    Some(format!("provider stdio failed ({e}); fell back to github")),
                ),
            }
        }
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
        fn search(&self, _: &str) -> ProviderResult<Vec<SearchItem>> {
            Err("offline".into())
        }
        fn org_repos(&self, _: &str) -> ProviderResult<Vec<String>> {
            Err("offline".into())
        }
        fn fetch_tree(&self, _: &str) -> ProviderResult<TreeResult> {
            Err("offline".into())
        }
        fn fetch_blob(&self, _: &str, _: &str) -> ProviderResult<Vec<u8>> {
            Err("offline".into())
        }
        fn search_code(&self, _: &str) -> ProviderResult<SearchCodeResult> {
            Err("offline".into())
        }
        fn clone_url(&self, _: &str) -> ProviderResult<String> {
            Err("offline".into())
        }
        fn web_url(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: Option<u32>,
            _: bool,
        ) -> ProviderResult<String> {
            Err("offline".into())
        }
        fn org_url(&self, _: &str) -> ProviderResult<String> {
            Err("offline".into())
        }
    }
    Arc::new(Offline)
}
