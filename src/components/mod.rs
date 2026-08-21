//! Component contract (PLAN.md §6). Components own state, emit Actions,
//! render into a caller-given Rect. See .agents/skills/rootle-component.

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
use ratatui::style::{Modifier, Style};

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

/// Pretty scrollbar embedded in the RIGHT border of a bordered box:
/// the track is the border itself (│, surface2), the thumb a bold
/// accent column (┃). No-op when the content fits. `total` = content
/// lines, `offset` = index of the top visible line.
pub(crate) fn scrollbar(
    frame: &mut Frame,
    outer: Rect,
    content_height: usize,
    total: usize,
    offset: usize,
    theme: &Theme,
) {
    let track = content_height;
    if total <= track || track == 0 || outer.height < 2 || outer.width == 0 {
        return;
    }
    let thumb = (track * track / total).max(1);
    let max_offset = total - track;
    let pos = offset.min(max_offset) * (track - thumb) / max_offset.max(1);
    let sem = &theme.semantic;
    let buf = frame.buffer_mut();
    for i in 0..track {
        let x = outer.x + outer.width - 1;
        let y = outer.y + 1 + i as u16;
        if x >= buf.area().width || y >= buf.area().height {
            break;
        }
        let cell = &mut buf[(x, y)];
        if i >= pos && i < pos + thumb {
            cell.set_symbol("┃");
            cell.set_style(
                Style::default()
                    .fg(sem.border_focused)
                    .add_modifier(Modifier::BOLD),
            );
        } else {
            cell.set_symbol("│");
            cell.set_style(Style::default().fg(sem.surface2));
        }
    }
}
