//! Component contract (PLAN.md §6). Components own state, emit Actions,
//! render into a caller-given Rect. See .agents/skills/ghx-component.

pub mod browser;
pub mod modeline;
pub mod pane;
pub mod preview;
pub mod search_popup;
pub mod vim_input;

use crate::action::Action;
use crate::theme::Theme;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

pub trait Component {
    fn handle_key(&mut self, key: KeyEvent) -> Action;
    fn update(&mut self, action: &Action);
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme);
}
