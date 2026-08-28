//! App events from background workers (GitHub API calls run on worker
//! threads; results return over this channel into the event loop).

use crate::provider::{ProviderError, SearchItem};

#[derive(Debug)]
pub enum AppEvent {
    SearchResults {
        gen_id: u64,
        items: Vec<SearchItem>,
    },
    SearchFailed {
        gen_id: u64,
        error: ProviderError,
    },
    OrgReposLoaded {
        org: String,
        repos: Vec<crate::provider::RepoInfo>,
    },
    OrgReposFailed {
        org: String,
        error: ProviderError,
    },
    TreeLoaded {
        owner: String,
        name: String,
        entries: Vec<crate::provider::TreeNode>,
        truncated: bool,
        branch: String,
    },
    TreeFailed {
        owner: String,
        name: String,
        error: ProviderError,
    },
    BlobLoaded {
        sha: String,
        name: String,
        bytes: Vec<u8>,
    },
    BlobFailed {
        sha: String,
        error: ProviderError,
    },
    /// Global search view results (plans/0002 §4): raw hits from the
    /// worker, styled on the UI thread.
    GlobalSearchResults {
        gen_id: u64,
        hits: Vec<crate::components::global_search::RawHit>,
        clipped: bool,
        /// v1.3: index freshness ("2026-08-20T14:00:00Z") for indexed
        /// backends — rendered next to the result count.
        index: Option<String>,
        /// v1.2-grammar (plans/0012 M1): hits the client-side
        /// subtraction filter removed.
        client_filtered: usize,
        /// Grammar tokens not expressible anywhere (title chip).
        unfiltered: Vec<String>,
    },
    /// Streamed batch (v1.3, plans/0011): raw hits as the provider
    /// emits them; styled on the UI thread, appended under `gen_id`.
    GlobalSearchDelta {
        gen_id: u64,
        hits: Vec<crate::components::global_search::RawHit>,
    },
    GlobalSearchFailed {
        gen_id: u64,
        error: ProviderError,
    },
    /// Lazy per-hit context (plans/0006 §1): blob fetched + located on
    /// a worker for the selected bare hit.
    HitContextLoaded {
        gen_id: u64,
        repo: String,
        path: String,
        sha: String,
        line: u32,
        preview: Vec<(u32, String)>,
        match_count: u32,
        query: String,
    },
    /// Lazy context fetch found no match text in the blob
    /// (plans/0008 §4): the hit flips to `unlocatable` instead of
    /// rendering stale forever.
    HitContextMissing {
        gen_id: u64,
        sha: String,
    },
    /// Cursor-rest debounce fired (plans/0008 §3): rapid selection
    /// moves collapse into this one dispatch for the hit the cursor
    /// finally rested on.
    HitContextDebounceFired {
        timer_gen: u64,
        hit: crate::components::global_search::SearchHit,
        query: String,
    },
    /// Lazy context fetch failed (plans/0008 §2): auth/throttle
    /// surfaces a status line; other kinds stay quiet (bare path
    /// remains, retry on revisit).
    HitContextFailed {
        gen_id: u64,
        sha: String,
        error: ProviderError,
    },
    /// Expanded file pane (plans/0012 M2): the hit's whole blob, raw
    /// bytes — the UI thread sanitizes + highlights at the boundary.
    HitFileLoaded {
        gen_id: u64,
        repo: String,
        path: String,
        sha: String,
        bytes: Vec<u8>,
    },
    HitFileFailed {
        gen_id: u64,
        sha: String,
        error: crate::provider::ProviderError,
    },
    CloneDone {
        ok: Vec<String>,
        failed: Vec<(String, String)>,
    },
    /// Org marks expanded to their repos; open the clone wizard with
    /// the combined list (plans/0004). v1.4: expanded repos carry
    /// their listing metadata (sort-by-pushed, grey archived).
    CloneExpanded {
        repos: Vec<crate::provider::RepoInfo>,
        errors: Vec<String>,
    },

    // Revision awareness (v1.5, plans/0016 M1). All scoped by
    // repo/path so late landings are dropped by the UI.
    /// Refs for the switcher popup.
    RefsLoaded {
        repo: String,
        refs: crate::provider::RepoRefs,
    },
    RefsFailed {
        repo: String,
        error: ProviderError,
    },
    /// Commit log for the history lens.
    LogLoaded {
        path: String,
        entries: Vec<crate::provider::LogEntry>,
        truncated: bool,
    },
    LogFailed {
        path: String,
        error: ProviderError,
    },
    /// Blame ranges for the blame lens.
    BlameLoaded {
        path: String,
        ranges: Vec<crate::provider::BlameRange>,
    },
    BlameFailed {
        path: String,
        error: ProviderError,
    },
    /// File bytes at a commit (open-at-commit from the history lens);
    /// the commit's subject/author/date dress the header band.
    BlobAtLoaded {
        path: String,
        ref_: String,
        sha: String,
        bytes: Vec<u8>,
        subject: String,
        author: String,
        date: String,
    },
    BlobAtFailed {
        path: String,
        error: ProviderError,
    },
    /// Startup update check (0017 M3): a newer release tag exists.
    /// `toast` (0018 M2): the once-a-day status nag was still due
    /// when the worker consumed it — the chip shows either way.
    UpdateAvailable {
        tag: String,
        toast: bool,
    },
    /// Declared-provider install finished (0019 M2): the app
    /// hot-swaps the provider and drops the consent popup.
    DeclarationInstalled {
        name: String,
    },
    /// Declared-provider install refused or failed (0019 M2): honest
    /// degraded mode — github fallback with a persistent status.
    DeclarationFailed {
        name: String,
        error: String,
    },
}

pub type AppTx = std::sync::mpsc::Sender<AppEvent>;
pub type AppRx = std::sync::mpsc::Receiver<AppEvent>;

pub fn channel() -> (AppTx, AppRx) {
    std::sync::mpsc::channel()
}
