//! Component contract (PLAN.md §6). Components own state, emit Actions,
//! render into a caller-given Rect. See .agents/skills/ghx-component.

pub mod browser;
pub mod clone_wizard;
pub mod command_line;
pub mod global_search;
pub mod keybinds_popup;
pub mod modeline;
pub mod pane;
pub mod preview;
pub mod search_popup;
pub mod settings_popup;
pub mod vim_input;

use crate::action::Action;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub trait Component {
    fn handle_key(&mut self, key: KeyEvent) -> Action;
    fn update(&mut self, action: &Action);
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme);
}

/// Centered percentage rect, shared by popups (search popup shell,
/// scope picker, future dialogs).
pub(crate) fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(vertical[1])[1]
}
