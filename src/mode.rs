//! Modal modes. The modeline chip is derived from these.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    /// Incremental `/` filter on the focused pane.
    Search,
    /// A text input is focused; sub-mode lives on `VimInput`.
    Insert,
    Normal,
    Leader,
    /// Later phases (multi-select).
    #[allow(dead_code)]
    Visual,
}

impl Mode {
    pub fn chip(self) -> &'static str {
        match self {
            Mode::Browse => "BROWSE",
            Mode::Search => "SEARCH",
            Mode::Insert => "INSERT",
            Mode::Normal => "NORMAL",
            Mode::Leader => "LEADER",
            Mode::Visual => "VISUAL",
        }
    }
}
