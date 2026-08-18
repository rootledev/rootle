//! Keymap tables: mode → key → Action, plus the hint rows derived from
//! the same source (PLAN.md §6). Dispatch and hints can never drift apart.

use crate::action::Action;
use crate::mode::Mode;
use ratatui::crossterm::event::KeyCode;

/// (key label, description) shown in the modeline / popup hint row.
pub fn hints(mode: Mode) -> &'static [(&'static str, &'static str)] {
    match mode {
        Mode::Browse => &[
            ("j/k", "move"),
            ("h/l", "out/in"),
            ("/", "filter"),
            ("␣", "leader"),
            ("q", "quit"),
        ],
        Mode::Search => &[("type", "filter"), ("enter", "commit"), ("esc", "cancel")],
        Mode::Insert => &[("enter", "submit"), ("tab", "results"), ("esc", "normal")],
        Mode::Normal => &[("i", "insert"), ("tab", "switch"), ("esc", "close")],
        Mode::Leader => &[("s", "search"), ("r", "reload"), ("q", "quit"), ("esc", "back")],
        Mode::Visual => &[],
    }
}

/// Browsing-mode dispatch. Text input modes are owned by `VimInput`;
/// SEARCHING-mode keys are owned by the pane filter input.
pub fn browsing(code: KeyCode) -> Action {
    match code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char(' ') => Action::Leader,
        KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
        KeyCode::Char('l') | KeyCode::Right => Action::DrillIn,
        KeyCode::Char('h') | KeyCode::Left => Action::DrillOut,
        KeyCode::Char('/') => Action::EnterSearch,
        KeyCode::Enter => Action::OpenSelected,
        KeyCode::Esc => Action::ClearFilter,
        _ => Action::Noop,
    }
}

pub fn leader(code: KeyCode) -> Action {
    match code {
        KeyCode::Char('s') => Action::LeaderSearch,
        KeyCode::Char('q') => Action::LeaderQuit,
        KeyCode::Esc => Action::ClosePopup, // reused: "back to previous mode"
        _ => Action::Noop,
    }
}
