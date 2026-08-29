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

/// The client's search render budget (protocol v1.4 advisory,
/// doc/provider-protocol.md): sent as `limit` on every `search/code`
/// so the provider stops scanning at ~N and sets `truncated: true`
/// instead of computing hits the view would clip. The view's render
/// cap is this same number.
pub const RENDER_BUDGET: usize = 500;

/// One repo in an org listing (protocol v1.4): the name plus whatever
/// metadata the backend reports. Everything past `name` is optional —
/// a provider with only names sends the string form on the wire and
/// every field here stays default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoInfo {
    pub name: String,
    pub description: Option<String>,
    pub private: bool,
    pub archived: bool,
    /// ISO-8601 last-push timestamp when the backend knows it.
    pub pushed_at: Option<String>,
}

impl RepoInfo {
    /// A bare name — selection-driven flows where no listing metadata
    /// exists.
    pub fn bare(name: impl Into<String>) -> Self {
        RepoInfo {
            name: name.into(),
            ..Default::default()
        }
    }
}

impl From<String> for RepoInfo {
    fn from(name: String) -> Self {
        RepoInfo::bare(name)
    }
}

impl From<&str> for RepoInfo {
    fn from(name: &str) -> Self {
        RepoInfo::bare(name)
    }
}

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

/// What a provider supports; the UI degrades on `false`. `file_search`
/// (v1.3) covers path-only search (the `path:` grammar); absent on the
/// wire it inherits `code_search` — a forge with filename search but
/// no global content index (Bitbucket Cloud, GitLab without Advanced
/// Search) says `code_search: false, file_search: true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub orgs: bool,
    pub code_search: bool,
    pub file_search: bool,
    /// v1.5 (plans/0016 M1): revision awareness — all default false;
    /// absent means default-branch-only, the honest answer for
    /// backends that can't answer (Bitbucket has no blame API).
    pub refs: bool,
    pub log: bool,
    pub blame: bool,
}

/// One ref (branch or tag) — `repo/refs` item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefInfo {
    pub name: String,
    pub sha: String,
    pub is_default: bool,
}

/// `repo/refs` reply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoRefs {
    pub branches: Vec<RefInfo>,
    pub tags: Vec<RefInfo>,
}

/// `repo/log` item — newest first on the wire.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct LogEntry {
    pub sha: String,
    pub subject: String,
    pub author: String,
    /// ISO-8601.
    pub date: String,
}

/// `repo/blame` range — 1-based inclusive lines, coalesced by sha.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct BlameRange {
    pub start_line: u32,
    pub end_line: u32,
    pub sha: String,
    pub author: String,
    pub date: String,
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

/// One code-search hit. `matches` are the matched substrings — an
/// empty vec is a legal **path-only hit** ("this file matched"). When
/// `matches` is non-empty the UI locates them in the blob for real
/// line numbers; `line`, when the provider knows it, is the anchor
/// used as-is (the first occurrence of a substring is often not the
/// occurrence that matched).
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
    /// v1.3: provider-known line number (1-based); `None` = unknown,
    /// the UI locates or anchors at 1.
    pub line: Option<u32>,
}

/// Code-search outcome metadata: the provider's own truncation signal
/// (plans/0008 §4) and, for indexed backends, when the index was
/// built (`located: false` covers the per-hit case; this is the
/// index-wide one — a lagging index is worth a badge next to the
/// results).
#[derive(Debug, Clone)]
pub struct SearchCodeResult {
    pub hits: Vec<CodeMatch>,
    pub truncated: bool,
    /// v1.3: e.g. "2026-08-20T14:00:00Z"; `None` = live or unknown.
    pub index_as_of: Option<String>,
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
    /// Repo names of an org/group, with listing metadata when the
    /// backend reports it (v1.4).
    fn org_repos(&self, org: &str) -> ProviderResult<Vec<RepoInfo>>;
    /// Full recursive tree of a repo — at `ref` (branch/tag/sha) when
    /// given (v1.5), else the default branch.
    fn fetch_tree(&self, repo: &str, ref_: Option<&str>) -> ProviderResult<TreeResult>;
    /// Blob bytes by content id.
    fn fetch_blob(&self, repo: &str, sha: &str) -> ProviderResult<Vec<u8>>;

    /// The repo's default-branch source as a gzip tarball — fuel for
    /// the local-grep fallback when a repo-scoped code search returns
    /// nothing (GitHub's index does not cover young/low-activity
    /// repos; the tree can't lie, the index can). Optional: the
    /// default refuses and the fallback is simply unavailable —
    /// external providers grow it when the wire protocol does.
    fn source_tarball(&self, repo: &str) -> ProviderResult<Vec<u8>> {
        let _ = repo;
        Err(ProviderError::other(
            "source tarball not supported by this provider",
        ))
    }
    fn refs(&self, repo: &str) -> ProviderResult<RepoRefs> {
        let _ = repo;
        Err(ProviderError::new(
            ErrorKind::Provider,
            "provider has no revision listing",
        ))
    }
    /// v1.5: commit log, newest first; `limit` rides the bounded-
    /// compute contract — stop at ~N, the bool is `truncated`
    /// (capability `log`).
    fn log(
        &self,
        repo: &str,
        path: Option<&str>,
        ref_: Option<&str>,
        limit: Option<usize>,
    ) -> ProviderResult<(Vec<LogEntry>, bool)> {
        let _ = (repo, path, ref_, limit);
        Err(ProviderError::new(
            ErrorKind::Provider,
            "provider has no commit log",
        ))
    }
    /// v1.5: file bytes + content id at a path and ref — the
    /// open-at-commit call (capability `log`'s companion).
    fn blob_at(
        &self,
        repo: &str,
        path: &str,
        ref_: Option<&str>,
    ) -> ProviderResult<(Vec<u8>, String)> {
        let _ = (repo, path, ref_);
        Err(ProviderError::new(
            ErrorKind::Provider,
            "provider cannot serve blobs at a ref",
        ))
    }
    /// v1.5: blame ranges, 1-based inclusive, coalesced (capability
    /// `blame`).
    fn blame(&self, repo: &str, path: &str, ref_: Option<&str>) -> ProviderResult<Vec<BlameRange>> {
        let _ = (repo, path, ref_);
        Err(ProviderError::new(
            ErrorKind::Provider,
            "provider has no blame",
        ))
    }
    /// Code search; `q` is the full query string with qualifiers.
    fn search_code(&self, q: &str) -> ProviderResult<SearchCodeResult>;

    /// Modeline icon: a builtin name ("github", "gitlab", "bitbucket",
    /// "folder" — rendered as its Nerd Font glyph when nerd_font is
    /// on) or a single literal glyph the terminal can render. The
    /// provider declares its own (handshake `icon`, protocol v1.3);
    /// the in-tree github provider owns the one rootle hardcodes.
    fn icon(&self) -> Option<String> {
        None
    }

    /// Progressive code search (protocol v1.3, plans/0011): `on_hits`
    /// may fire from any thread, any number of times, strictly before
    /// this call returns. When the provider streamed, the result is
    /// metadata-only — `hits` empty, `truncated` authoritative.
    /// Default: one `search_code` call, one `on_hits` batch — every
    /// provider streams; page-shaped backends stream page-by-page.
    fn search_code_progressive(
        &self,
        q: &str,
        on_hits: &(dyn Fn(&[CodeMatch]) + Send + Sync),
    ) -> ProviderResult<SearchCodeResult> {
        let result = self.search_code(q)?;
        on_hits(&result.hits);
        Ok(SearchCodeResult {
            hits: Vec::new(),
            truncated: result.truncated,
            index_as_of: result.index_as_of,
        })
    }

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
        end: Option<u32>,
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

/// A config-declared provider that isn't installed on this machine
/// (plans/0019 M2) — surfaced to the app for the consent flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// The receipt/install name (`gitlab`, `rootle-gitlab`, …).
    pub name: String,
    /// "owner/repo" — a releases-API source only; a config naming a
    /// plain-HTTP tarball never reaches a Declaration.
    pub repo: String,
    /// Pin: install exactly this tag.
    pub tag: Option<String>,
    /// Integrity pin: verify the tarball against this sha256 too.
    pub sha: Option<String>,
}

/// How `build()` landed (plans/0019 M2, 0022).
#[derive(Debug)]
pub enum BuildOutcome {
    /// The configured provider is up.
    Ready,
    /// Up with a warning (github fallback when the configured one
    /// failed — never silent, never blocking; 0022 M1 makes the
    /// notice sticky).
    Warn(String),
    /// 0022 M2: the configured provider exists but won't start — the
    /// health prompt (retry / browse github / edit config).
    Health(HealthIssue),
    /// A declared provider is missing; github is carrying the session
    /// pending the consent popup.
    Missing(Declaration),
}

/// A provider that exists on disk (or in config) but fails to start —
/// the health prompt's payload (0022 M2).
#[derive(Debug, Clone)]
pub struct HealthIssue {
    /// What the config names (display name).
    pub name: String,
    /// Why it failed (spawn/parse error text).
    pub error: String,
    /// Does retry make sense? false for malformed/tarball kinds —
    /// retrying a typo fixes nothing.
    pub retryable: bool,
}

/// Build the configured provider. Invalid/unsupported config falls
/// back to GitHub (with a warning for the status line) — a provider
/// misconfiguration must never block startup.
pub fn build(config: &crate::config::Config) -> (Arc<dyn Provider>, BuildOutcome) {
    match config.provider.kind.as_str() {
        "github" => (
            Arc::new(github::GitHubProvider::new(config.cache.max_mb)),
            BuildOutcome::Ready,
        ),
        "stdio" => {
            let (provider, warn) = try_spawn_stdio(config, config.provider.command.clone(), None);
            match provider {
                Ok(p) => (p, warn.map_or(BuildOutcome::Ready, BuildOutcome::Warn)),
                Err(e) => (
                    github_fallback(config),
                    BuildOutcome::Health(HealthIssue {
                        name: "stdio".into(),
                        error: e,
                        retryable: true,
                    }),
                ),
            }
        }
        other => build_declared(config, other),
    }
}

/// The declared kind (plans/0019 M2): a receipt name, a bare
/// first-party name (`gitlab` → `rootledev/rootle-gitlab` via the
/// Ref grammar's rootle- convention), or an `owner/repo` slug.
/// Installed → spawn the `current` binary; missing → the consent
/// flow. Plain-HTTP tarball refs are never auto-fetched.
fn build_declared(config: &crate::config::Config, kind: &str) -> (Arc<dyn Provider>, BuildOutcome) {
    let r = match manager::Ref::parse(kind) {
        Ok(r) => r,
        Err(e) => {
            return (
                github_fallback(config),
                BuildOutcome::Health(HealthIssue {
                    name: kind.to_string(),
                    error: e.to_string(),
                    retryable: false,
                }),
            );
        }
    };
    if r.tarball.is_some() {
        return (
            github_fallback(config),
            BuildOutcome::Health(HealthIssue {
                name: kind.to_string(),
                error: "kind names a plain-HTTP tarball — install-and-pin only (run `rootle provider install` with the URL)".into(),
                retryable: false,
            }),
        );
    }
    let Ok(m) = manager::Manager::new() else {
        return (
            github_fallback(config),
            BuildOutcome::Warn("no provider data dir; using github".into()),
        );
    };
    match m.current_binary(&r.name) {
        Some(bin) => {
            // Extra argv (beyond the binary) from a hand-written
            // config still rides along.
            let mut argv = vec![bin.display().to_string()];
            argv.extend(config.provider.command.iter().cloned());
            match try_spawn_stdio(config, argv, Some(r.name.clone())).0 {
                Ok(p) => (p, BuildOutcome::Ready),
                Err(e) => (
                    github_fallback(config),
                    BuildOutcome::Health(HealthIssue {
                        name: kind.to_string(),
                        error: e,
                        retryable: true,
                    }),
                ),
            }
        }
        None => (
            github_fallback(config),
            BuildOutcome::Missing(Declaration {
                name: r.name,
                repo: r.repo,
                tag: config.provider.tag.clone().or(r.tag),
                sha: config.provider.sha.clone(),
            }),
        ),
    }
}

/// Spawn an installed declared provider by name — the 0019 M2
/// hot-swap after the consent install lands.
pub fn spawn_installed(
    config: &crate::config::Config,
    name: &str,
) -> Result<Arc<dyn Provider>, String> {
    let bin = manager::Manager::new()
        .ok()
        .and_then(|m| m.current_binary(name))
        .ok_or_else(|| format!("{name} installed but its current binary is missing"))?;
    let mut argv = vec![bin.display().to_string()];
    argv.extend(config.provider.command.iter().cloned());
    try_spawn_stdio(config, argv, Some(name.to_string())).0
}

fn github_fallback(config: &crate::config::Config) -> Arc<dyn Provider> {
    Arc::new(github::GitHubProvider::new(config.cache.max_mb))
}

/// The stdio spawn shared by the `stdio` kind and declared providers.
/// Returns (result, warning): the warning covers an unrecognized
/// `stderr` value (a typo shouldn't silently disable debugging).
fn try_spawn_stdio(
    config: &crate::config::Config,
    argv: Vec<String>,
    cache_name: Option<String>,
) -> (Result<Arc<dyn Provider>, String>, Option<String>) {
    let stderr = config.provider.stderr.trim();
    let inherit = stderr == "inherit";
    let warn = (!inherit && !stderr.is_empty() && stderr != "null").then(|| {
        format!(
            "provider stderr {stderr:?} not recognized (use \"inherit\" or \"null\"); discarding child stderr"
        )
    });
    // The user's cache budget and this provider's subtree travel in
    // every initialize (protocol v1.2, advisory) — one [cache] max_mb
    // knob governs every backend.
    let cache_bytes = config.cache.max_mb * 1024 * 1024;
    let cache_dir = dirs::cache_dir().map(|d| {
        d.join("rootle")
            .join("providers")
            .join(cache_name.unwrap_or_else(|| name_from_command(&argv)))
    });
    match stdio::StdioProvider::spawn_with_cache(
        &argv,
        std::time::Duration::from_millis(config.provider.timeout_ms),
        inherit,
        cache_bytes,
        cache_dir,
    ) {
        Ok(p) => (Ok(Arc::new(p)), warn),
        Err(e) => (Err(e.to_string()), warn),
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
                file_search: false,
                // Tests inject the v1.5 events directly (the calls
                // themselves error offline) — declare the caps so the
                // lenses open.
                refs: true,
                log: true,
                blame: true,
            }
        }
        fn search(&self, _: &str) -> ProviderResult<Vec<SearchItem>> {
            Err("offline".into())
        }
        fn org_repos(&self, _: &str) -> ProviderResult<Vec<RepoInfo>> {
            Err("offline".into())
        }
        fn fetch_tree(&self, _: &str, _: Option<&str>) -> ProviderResult<TreeResult> {
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
