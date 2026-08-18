//! Modal modes. The modeline chip is derived from these.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browsing,
    /// Incremental `/` filter on the focused pane.
    Searching,
    /// A text input is focused; sub-mode lives on `VimInput`.
    InputInsert,
    InputNormal,
    Leader,
    /// Later phases (multi-select).
    #[allow(dead_code)]
    Visual,
}

impl Mode {
    pub fn chip(self) -> &'static str {
        match self {
            Mode::Browsing => "BROWSING",
            Mode::Searching => "SEARCHING",
            Mode::InputInsert => "INSERT",
            Mode::InputNormal => "NORMAL",
            Mode::Leader => "LEADER",
            Mode::Visual => "VISUAL",
        }
    }
}
