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

    // Selection outcomes
    RepoSelected {
        owner: String,
        name: String,
    },
    OpenSelected,
}
