//! Modal modes. The modeline chip is derived from these.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    /// Incremental `/` filter on the focused pane.
    Search,
    /// Vim-style find-in-file over the preview (`␣ /`, plans/0007 §3).
    Find,
    /// A text input is focused; sub-mode lives on `VimInput`.
    Insert,
    Normal,
    Leader,
    /// Later phases (multi-select).
    #[allow(dead_code)]
    Visual,
    /// File-history lens over the preview pane (plans/0016 M1b).
    History,
}

impl Mode {
    pub fn chip(self) -> &'static str {
        match self {
            Mode::Browse => "BROWSE",
            Mode::Search => "SEARCH",
            Mode::Find => "FIND",
            Mode::Insert => "INSERT",
            Mode::Normal => "NORMAL",
            Mode::Leader => "LEADER",
            Mode::Visual => "VISUAL",
            Mode::History => "HISTORY",
        }
    }
}
