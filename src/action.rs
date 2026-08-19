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
    PreviewScrollUp,
    PreviewScrollDown,

    // Pane filter (SEARCHING mode)
    EnterSearch,
    CommitFilter,
    ClearFilter,

    // Leader
    Leader,
    LeaderSearch,
    LeaderQuit,

    // Popup
    ClosePopup,

    // Search popup ↔ GitHub backend
    SearchSubmitted(String),
    SearchResults {
        items: Vec<crate::github::SearchItem>,
    },
    SearchFailed {
        message: String,
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
        message: String,
    },

    // Repo tree loading
    LoadRepoTree {
        owner: String,
        name: String,
    },
    TreeLoaded {
        owner: String,
        name: String,
        entries: Vec<crate::github::types::TreeNode>,
        truncated: bool,
    },
    TreeFailed {
        owner: String,
        name: String,
        message: String,
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
        message: String,
    },

    // Selection outcomes
    RepoSelected {
        owner: String,
        name: String,
    },
    OpenSelected,
}
