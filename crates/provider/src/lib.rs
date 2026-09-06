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

/// The client's search render budget (protocol v1.4 advisory,
/// doc/provider-protocol.md): sent as `limit` on every `search/code`
/// so the provider stops scanning at ~N and sets `truncated: true`
/// instead of computing hits the view would clip. The view's render
/// cap is this same number.
pub const RENDER_BUDGET: usize = 500;

pub mod id;

pub use id::{Generation, GitRef, RepoId, Sha};

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
    /// v1.6 (plans/0028): commit detail — full message, changed files,
    /// per-file unified hunks. Default false, same family as the v1.5
    /// trio.
    pub commit: bool,
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

/// v1.6 (plans/0028): one changed file in a commit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct CommitFile {
    /// Repo-relative path after the change.
    pub path: String,
    /// `added` | `removed` | `modified` | `renamed` (wire: those
    /// lowercase names; unknown values degrade to `modified`).
    pub status: FileStatus,
    /// Line counts when the backend reports them (wire optional).
    #[serde(default)]
    pub additions: Option<u32>,
    #[serde(default)]
    pub deletions: Option<u32>,
    /// The file's unified hunks (hunk headers + body, no file
    /// headers), absent for binary files.
    #[serde(default)]
    pub patch: Option<String>,
    /// `renamed` only: the path before the change.
    #[serde(default)]
    pub previous_path: Option<String>,
}

/// A changed file's kind (wire `status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    #[default]
    Modified,
    Added,
    Removed,
    Renamed,
}

impl FileStatus {
    /// The one-glyph row marker for the changed-files list.
    pub fn glyph(self) -> &'static str {
        match self {
            FileStatus::Modified => "~",
            FileStatus::Added => "+",
            FileStatus::Removed => "-",
            FileStatus::Renamed => "→",
        }
    }

    /// The filterable word for the status (`/status:`-ish matching
    /// rides the plain substring contract).
    pub fn label(self) -> &'static str {
        match self {
            FileStatus::Modified => "modified",
            FileStatus::Added => "added",
            FileStatus::Removed => "removed",
            FileStatus::Renamed => "renamed",
        }
    }
}

/// v1.6: `repo/commit` reply — one commit, inspectable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct CommitDetail {
    pub sha: String,
    pub author: String,
    /// ISO-8601.
    pub date: String,
    /// Full commit message (subject + body), sanitized at the UI
    /// boundary like every network string.
    pub message: String,
    /// Parent shas when the backend reports them.
    #[serde(default)]
    pub parents: Vec<String>,
    pub files: Vec<CommitFile>,
}

impl CommitDetail {
    /// Total added/deleted lines across files (unknown counts read 0).
    pub fn line_stats(&self) -> (u32, u32) {
        self.files.iter().fold((0, 0), |(a, d), f| {
            (a + f.additions.unwrap_or(0), d + f.deletions.unwrap_or(0))
        })
    }
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
    fn fetch_tree(&self, repo: &RepoId, ref_: Option<&GitRef>) -> ProviderResult<TreeResult>;
    /// Blob bytes by content id.
    fn fetch_blob(&self, repo: &RepoId, sha: &Sha) -> ProviderResult<Vec<u8>>;

    /// The repo's default-branch source as a gzip tarball — fuel for
    /// the local-grep fallback when a repo-scoped code search returns
    /// nothing (GitHub's index does not cover young/low-activity
    /// repos; the tree can't lie, the index can). Optional: the
    /// default refuses and the fallback is simply unavailable —
    /// external providers grow it when the wire protocol does.
    fn source_tarball(&self, repo: &RepoId) -> ProviderResult<Vec<u8>> {
        let _ = repo;
        Err(ProviderError::other(
            "source tarball not supported by this provider",
        ))
    }
    fn refs(&self, repo: &RepoId) -> ProviderResult<RepoRefs> {
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
        repo: &RepoId,
        path: Option<&str>,
        ref_: Option<&GitRef>,
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
        repo: &RepoId,
        path: &str,
        ref_: Option<&GitRef>,
    ) -> ProviderResult<(Vec<u8>, Sha)> {
        let _ = (repo, path, ref_);
        Err(ProviderError::new(
            ErrorKind::Provider,
            "provider cannot serve blobs at a ref",
        ))
    }
    /// v1.5: blame ranges, 1-based inclusive, coalesced (capability
    /// `blame`).
    fn blame(
        &self,
        repo: &RepoId,
        path: &str,
        ref_: Option<&GitRef>,
    ) -> ProviderResult<Vec<BlameRange>> {
        let _ = (repo, path, ref_);
        Err(ProviderError::new(
            ErrorKind::Provider,
            "provider has no blame",
        ))
    }
    /// v1.6 (plans/0028): one commit's detail — message, changed
    /// files, unified hunks (capability `commit`).
    fn commit(&self, repo: &RepoId, sha: &Sha) -> ProviderResult<CommitDetail> {
        let _ = (repo, sha);
        Err(ProviderError::new(
            ErrorKind::Provider,
            "provider has no commit detail",
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
    fn clone_url(&self, repo: &RepoId) -> ProviderResult<String>;
    /// Browser URL for yank (␣ y): repo root, or a path inside it.
    /// `is_file` picks the grammar (GitHub: blob vs tree); `line`
    /// adds a fragment when Some. `branch` is `None` for the default
    /// branch (the provider resolves it).
    fn web_url(
        &self,
        repo: &RepoId,
        path: &str,
        branch: Option<&GitRef>,
        line: Option<u32>,
        end: Option<u32>,
        is_file: bool,
    ) -> ProviderResult<String>;

    /// Browser URL for an org/group page.
    fn org_url(&self, org: &str) -> ProviderResult<String>;
}
pub mod paths;

/// Env-gated trace sink (`ROOTLE_TRACE=<path>`): timestamped appends,
/// never a behavior change. Shared by the app and the provider
/// crates (github client, transports) so one knob traces everything.
pub fn trace(msg: &str) {
    if let Ok(path) = std::env::var("ROOTLE_TRACE") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{:?} {msg}", std::time::SystemTime::now());
        }
    }
}
