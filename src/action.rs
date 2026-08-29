//! Central action enum — components emit these, the root dispatcher routes.

/// Worker-progress state for the declared-provider consent popup
/// (plans/0019 M2): `Installing` while the verified download runs,
/// `Failed` once it refuses — the popup shows why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationState {
    Installing,
    Failed(String),
}

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
    /// `␣ b` — revision switcher (branches/tags), plans/0016 M1a.
    LeaderRefs,
    /// `␣ p` — the preview pane takes the keyboard (plans/0016 M1).
    LeaderPreview,
    /// Esc/q out of the preview submode.
    ExitPreview,
    /// `␣ p h` — file history lens over the preview, plans/0016 M1b.
    LeaderHistory,
    /// `␣ p b` — blame lens over the preview (margin runs), M1c.
    BlameToggle,
    /// `Y` in a file pane — copy content (the visual selection, else
    /// the cursor line) to the clipboard (GitHub's copy button).
    PreviewCopy,
    /// Enter in the preview submode: editor — or the line's commit in
    /// the history lens while blaming.
    PreviewEnter,
    LeaderQuit,

    // Revision mocks (plans/0016 M1): the refs popup live-previews the
    // crumb, Enter commits, Esc reverts; the history lens navigates
    // commits and "opens" a revision (mock: status line, no fetch).
    RefsPreview(String),
    RefsCommit(String),
    HistoryUp,
    HistoryDown,
    HistoryOpen,
    HistoryClose,

    // Declared-provider lifecycle (plans/0019 M2): the consent popup
    // asks, the app spawns the verified install, the events land.
    /// `y` in the repo search popup: yank the selected entry's URL
    /// (0021 M3 hygiene — no silent dead keys in any context).
    SearchYank,
    DeclarationAccept,
    /// 0022 M2 health prompt: retry the failed spawn once.
    DeclarationRetry,
    /// 0022 M2 health prompt: open the config file in the editor.
    DeclarationEditConfig,
    DeclarationDecline,
    /// Worker state landing back in the popup (installing / failed).
    DeclarationState(DeclarationState),
    /// `/` in the history lens: commit-list filter session.
    HistoryFilterBegin,
    /// `y` in the history lens: yank the file URL anchored to the
    /// commit — the permalink that never rots (plans/0016 M1b).
    HistoryYank,

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
        /// v1.3: index freshness for indexed backends; `None` = live
        /// or unknown.
        index: Option<String>,
        /// plans/0012 M1: hits the client-side grammar filter removed.
        client_filtered: usize,
        /// Grammar tokens rootle couldn't express anywhere.
        unfiltered: Vec<String>,
    },
    /// Streamed batch (v1.3, plans/0011): merged into the result set
    /// by file identity; selection survives.
    GlobalSearchDelta {
        hits: Vec<crate::components::global_search::SearchHit>,
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
    /// Expand the hit into the full-file pane (plans/0012 M2): fetch
    /// the whole blob by repo+sha — cache-first, so a hit whose
    /// context the lazy locate already fetched is free.
    LoadHitFile {
        hit: Box<crate::components::global_search::SearchHit>,
    },
    /// The expanded pane's blob landed, styled on the UI thread like
    /// every other blob (sanitize + highlight at the boundary).
    HitFileLoaded {
        repo: String,
        path: String,
        sha: String,
        lang: String,
        lines: Vec<ratatui::text::Line<'static>>,
    },
    /// The expanded pane's blob fetch failed (or the blob is binary).
    HitFileFailed {
        sha: String,
        error: crate::provider::ProviderError,
    },

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
        repos: Vec<crate::provider::RepoInfo>,
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
