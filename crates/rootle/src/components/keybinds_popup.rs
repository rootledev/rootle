//! Keybinds popup (`?`, plans/0003 §2): every binding, derived from
//! the keymap tables — the same source of truth as dispatch, so the
//! list can never drift. Settings-style layout: the modes in a
//! sidebar as their own chips (active one filled, with the binding
//! count), that mode's bindings on the right as keycap chips +
//! descriptions. Tab/h/l switch modes, j/k scroll, Esc closes. The
//! title row carries the app version.

use super::list_view::{FilterOutcome, ListFilter, ScrollMovement, Viewport};
use super::modeline::mode_color;
use crate::action::Action;
use crate::keymap;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Sidebar column: marker + mode chip + binding count.
const SIDEBAR: u16 = 15;
/// Keycap column inside the content block.
const KEYCAP: usize = 10;

const MODES: [Mode; 10] = [
    Mode::Browse,
    Mode::Search,
    Mode::Find,
    Mode::Insert,
    Mode::Normal,
    Mode::Leader,
    Mode::Visual,
    Mode::History,
    Mode::Preview,
    Mode::Commit,
];

pub struct KeybindsPopup {
    mode: usize,
    viewport: Viewport,
    filter: ListFilter,
}

impl KeybindsPopup {
    pub(crate) fn diagnostics(&self, full: bool) -> serde_json::Value {
        serde_json::json!({"mode":MODES[self.mode].chip(), "viewport":self.viewport.diagnostics(),
            "filter":self.filter.diagnostics(full)})
    }

    pub fn effective_mode(&self) -> Mode {
        if self.filter.active() {
            Mode::Search
        } else {
            Mode::Browse
        }
    }
    pub fn new() -> Self {
        KeybindsPopup {
            mode: 0,
            viewport: Viewport::default(),
            filter: ListFilter::default(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        use crate::keymap::{ListCommand, ListContext, list_command};
        if self.filter.active() {
            if self.filter.handle_key(key) != FilterOutcome::Unchanged {
                self.viewport.reset();
            }
            return Action::Noop;
        }
        match list_command(ListContext::Help, key) {
            Some(ListCommand::NextSection) => {
                self.mode = (self.mode + 1) % MODES.len();
                self.viewport.reset();
                Action::Noop
            }
            Some(ListCommand::PreviousSection) => {
                self.mode = (self.mode + MODES.len() - 1) % MODES.len();
                self.viewport.reset();
                Action::Noop
            }
            Some(ListCommand::Next) => {
                self.viewport.scroll(ScrollMovement::Down);
                Action::Noop
            }
            Some(ListCommand::Previous) => {
                self.viewport.scroll(ScrollMovement::Up);
                Action::Noop
            }
            Some(ListCommand::First) => {
                self.viewport.scroll(ScrollMovement::Top);
                Action::Noop
            }
            Some(ListCommand::Last) => {
                self.viewport.scroll(ScrollMovement::Bottom);
                Action::Noop
            }
            Some(ListCommand::Filter) => {
                self.filter.begin();
                Action::Noop
            }
            Some(ListCommand::Cancel) if self.filter.clear() => {
                self.viewport.reset();
                Action::Noop
            }
            Some(ListCommand::Cancel) => Action::ClosePopup,
            _ => Action::Noop,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let popup = super::centered_clamped(area, 60, 70, 36, 12);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                if self.filter.active() || !self.filter.is_empty() {
                    format!(" keybindings /{} ", self.filter.text())
                } else {
                    " keybindings ".to_string()
                },
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_top(
                Line::from(Span::styled(
                    format!(" rootle v{VERSION} "),
                    Style::default().fg(sem.hint),
                ))
                .right_aligned(),
            )
            .title_bottom(Span::styled(
                keymap::hint_row(keymap::list_hints(
                    keymap::ListContext::Help,
                    self.filter.active(),
                )),
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR), Constraint::Min(1)])
            .split(inner);
        self.render_sidebar(frame, cols[0], theme);
        self.render_mode(frame, cols[1], theme);
    }

    /// Mode list as their own chips: the active one is the filled chip
    /// (mode color background) with the ▸ marker, the rest render as
    /// dim colored outlines. The right column counts the bindings.
    fn render_sidebar(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let mut lines = vec![Line::raw("")];
        for (i, mode) in MODES.iter().enumerate() {
            let active = i == self.mode;
            let color = mode_color(*mode, sem);
            let count = keymap::hints(*mode).len().to_string();
            let (chip, bg) = if active {
                (
                    Style::default()
                        .fg(sem.crust)
                        .bg(color)
                        .add_modifier(Modifier::BOLD),
                    sem.selection_bg,
                )
            } else {
                (Style::default().fg(color), sem.mantle)
            };
            let marker = if active { "▸ " } else { "  " };
            let label = format!(" {} ", mode.chip());
            let used = marker.width() + label.width() + count.width();
            let pad = (area.width as usize).saturating_sub(used);
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(sem.border_focused).bg(bg)),
                Span::styled(label, chip.bg(bg)),
                Span::styled(
                    format!("{}{}", " ".repeat(pad), count),
                    Style::default()
                        .fg(if active { sem.subtext0 } else { sem.overlay0 })
                        .bg(bg),
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    /// The active mode's bindings in a base-colored block titled with
    /// the mode's chip: keycap chips in a fixed column, descriptions
    /// after. Scrolls (border scrollbar) when it can't fit.
    fn render_mode(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let mode = MODES[self.mode];
        let color = mode_color(mode, sem);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_unfocused))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(
                format!(" {} ", mode.chip()),
                Style::default()
                    .fg(sem.crust)
                    .bg(color)
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let rows: Vec<Line> = keymap::hints(mode)
            .iter()
            .filter(|(key, description)| {
                self.filter.matches(key) || self.filter.matches(description)
            })
            .map(|(key, desc)| {
                let cap = format!(" {key} ");
                let pad = KEYCAP.saturating_sub(cap.width());
                Line::from(vec![
                    Span::styled(
                        cap,
                        Style::default()
                            .fg(sem.text)
                            .bg(sem.surface0)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{}{}", " ".repeat(pad), desc),
                        Style::default().fg(sem.subtext0).bg(sem.base),
                    ),
                ])
            })
            .collect();
        self.viewport.render(frame, area, inner, rows, None, theme);
    }
}

impl Default for KeybindsPopup {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Snapshot of the popup states: sidebar chips + counts, keycap
    /// rows for the active mode, version in the title (skill:
    /// TestBackend per component).
    #[test]
    fn render_shows_chips_keycaps_and_version() {
        let mut p = KeybindsPopup::new();
        let theme = Theme::catppuccin_mocha();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| p.render(f, f.area(), &theme)).unwrap();
        let screen = {
            let buf = terminal.backend().buffer();
            (0..buf.area.height)
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        // Sidebar: every mode's chip with its binding count; BROWSE
        // active by default.
        for mode in MODES {
            assert!(screen.contains(mode.chip()), "{} chip missing", mode.chip());
        }
        assert!(
            screen.contains("rootle v"),
            "version missing from the title"
        );
        assert!(screen.contains("move"), "browse bindings missing");
        assert!(screen.contains("quit"), "browse bindings missing");

        // Tab walks the modes; the content block shows that mode's
        // bindings (leader has the leader table).
        for _ in 0..5 {
            p.handle_key(key(KeyCode::Tab));
        }
        terminal.draw(|f| p.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(screen.contains("clear marks"), "leader bindings missing");
        assert!(!screen.contains("VISUAL filter"), "only one mode at a time");
    }

    #[test]
    fn tab_cycles_modes_and_esc_closes() {
        let mut p = KeybindsPopup::new();
        assert_eq!(p.mode, 0);
        for _ in 0..MODES.len() {
            p.handle_key(key(KeyCode::Tab));
        }
        assert_eq!(p.mode, 0, "Tab must wrap");
        p.handle_key(key(KeyCode::BackTab));
        assert_eq!(p.mode, MODES.len() - 1);
        assert_eq!(p.handle_key(key(KeyCode::Esc)), Action::ClosePopup);
        assert_eq!(p.handle_key(key(KeyCode::Char('?'))), Action::ClosePopup);
    }

    #[test]
    fn long_mode_label_keeps_its_full_binding_count_visible() {
        let popup = KeybindsPopup::new();
        let mut terminal = Terminal::new(TestBackend::new(SIDEBAR, 12)).unwrap();
        terminal
            .draw(|frame| popup.render_sidebar(frame, frame.area(), &Theme::catppuccin_mocha()))
            .unwrap();
        let screen = crate::headless::buffer_text(terminal.backend().buffer());
        let row = screen.lines().find(|row| row.contains("PREVIEW")).unwrap();
        let visible_count = row
            .split_whitespace()
            .last()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(visible_count, keymap::hints(Mode::Preview).len());
    }
}
