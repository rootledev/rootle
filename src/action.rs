//! Central action enum — components emit these, the root dispatcher routes.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Noop,
    Quit,

    // Navigation
    MoveUp,
    MoveDown,
    DrillIn,
    DrillOut,
    PreviewLineUp,
    PreviewLineDown,

    // Pane filter (SEARCHING mode)
    EnterSearch,
    CommitFilter,
    ClearFilter,

    // Find-in-file (plans/0007 §3): `␣ /` opens FIND over the preview;
    // n/N cycle a committed search.
    LeaderFindInFile,
    /// FIND-mode keystroke changed the query — recompute matches live.
    UpdateFind,
    CommitFind,
    CancelFind,
    FindNext,
    FindPrev,

    // Leader
    Leader,
    LeaderSearch,
    LeaderFileFind,
    LeaderGrep,
    LeaderQuit,

    // Popup
    ClosePopup,

    // Overlays & command layer (plans/0003, plans/0004)
    KeybindsPopup,
    CommandLine,
    RunCommand(String),
    Visual,
    ExitVisual,
    ToggleSelect,
    /// `␣ y` — yank the context's remote URL (plans/0003 §1).
    LeaderYank,
    /// `␣ c` — clear all VISUAL marks.
    ClearMarks,
    /// `␣ d` — delete marked orgs from the browser (+ state).
    DeleteMarked,
    /// `␣ r` — reload the open repo's tree (or the org's repos).
    LeaderReload,
    /// Settings popup closed with edits: persist + hot reload.
    ApplySettings(crate::config::Config),
    /// Clone wizard: run the clones (repos + destination).
    RunClone {
        repos: Vec<String>,
        dest: std::path::PathBuf,
    },

    // Global search view (plans/0002-v0.2)
    CloseSearchView,
    GlobalSearchSubmitted {
        kind: crate::components::global_search::SearchKind,
        query: String,
        scope: String,
        extension: String,
    },
    GlobalSearchResults {
        hits: Vec<crate::components::global_search::SearchHit>,
        /// Provider-truncated or client-capped result set (plans/0008
        /// §4) — complete and clipped sets are distinguishable.
        clipped: bool,
    },
    GlobalSearchFailed {
        error: crate::provider::ProviderError,
    },
    /// Cursor landed on a hit without preview lines but with a sha —
    /// fetch its blob and locate the context lazily (plans/0006 §1).
    LoadHitContext {
        hit: Box<crate::components::global_search::SearchHit>,
        query: String,
    },
    HitContextLoaded {
        repo: String,
        path: String,
        sha: String,
        line: u32,
        preview: Vec<(u32, ratatui::text::Line<'static>)>,
        match_count: u32,
    },
    OpenSearchHit(crate::components::global_search::SearchHit),

    // Search popup ↔ GitHub backend
    SearchSubmitted(String),
    SearchResults {
        items: Vec<crate::provider::SearchItem>,
    },
    SearchFailed {
        error: crate::provider::ProviderError,
    },

    // Org loading
    OrgSelected(String),
    LoadOrgRepos(String),
    OrgReposLoaded {
        org: String,
        repos: Vec<String>,
    },
    OrgReposFailed {
        org: String,
        error: crate::provider::ProviderError,
    },

    // Repo tree loading
    LoadRepoTree {
        owner: String,
        name: String,
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
        error: crate::provider::ProviderError,
    },

    // Blob preview
    LoadBlob {
        sha: String,
        name: String,
    },
    BlobLoaded {
        sha: String,
        name: String,
        bytes: Vec<u8>,
    },
    BlobFailed {
        sha: String,
        error: crate::provider::ProviderError,
    },
    /// Lazy hit-context fetch failed (plans/0008 §2).
    HitContextFailed {
        sha: String,
        error: crate::provider::ProviderError,
    },
    /// Blob fetched but the match text isn't in it (plans/0008 §4).
    HitContextMissing {
        sha: String,
    },
    /// Cursor-rest debounce fired (plans/0008 §3): dispatch the lazy
    /// context fetch for the hit the cursor finally rested on.
    HitContextDebounceFired {
        timer_gen: u64,
        hit: crate::components::global_search::SearchHit,
        query: String,
    },

    // Selection outcomes
    RepoSelected {
        owner: String,
        name: String,
    },
    OpenSelected,
}
