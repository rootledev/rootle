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

    // Selection outcomes
    RepoSelected { owner: String, name: String },
    OpenSelected,
}
