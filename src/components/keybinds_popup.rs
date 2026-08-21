//! Keybinds popup (`?`, plans/0003 §2): every binding, derived from
//! the keymap tables — the same source of truth as dispatch, so the
//! list can never drift. Settings-style layout: the modes in a
//! sidebar as their own chips (active one filled, with the binding
//! count), that mode's bindings on the right as keycap chips +
//! descriptions. Tab/h/l switch modes, j/k scroll, Esc closes. The
//! title row carries the app version.

use super::modeline::mode_color;
use crate::action::Action;
use crate::keymap;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Sidebar column: marker + mode chip + binding count.
const SIDEBAR: u16 = 15;
/// Keycap column inside the content block.
const KEYCAP: usize = 10;

const MODES: [Mode; 6] = [
    Mode::Browse,
    Mode::Search,
    Mode::Insert,
    Mode::Normal,
    Mode::Leader,
    Mode::Visual,
];

pub struct KeybindsPopup {
    mode: usize,
    scroll: u16,
}

impl KeybindsPopup {
    pub fn new() -> Self {
        KeybindsPopup { mode: 0, scroll: 0 }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
                self.mode = (self.mode + 1) % MODES.len();
                self.scroll = 0;
                Action::Noop
            }
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
                self.mode = (self.mode + MODES.len() - 1) % MODES.len();
                self.scroll = 0;
                Action::Noop
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll = self.scroll.saturating_add(1);
                Action::Noop
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                Action::Noop
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => Action::ClosePopup,
            _ => Action::Noop,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let popup = super::centered(area, 60, 70);
        frame.render_widget(Clear, popup);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                " keybindings ",
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
                " tab/h/l mode · j/k scroll · esc close ",
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
            let count = keymap::hints(*mode).len();
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
            let used = 2 + 8 + 1 + 1; // marker + chip + gap + count digit
            let pad = (area.width as usize).saturating_sub(used);
            lines.push(Line::from(vec![
                Span::styled(
                    if active { "▸ " } else { "  " },
                    Style::default().fg(sem.border_focused).bg(bg),
                ),
                Span::styled(format!(" {} ", mode.chip()), chip.bg(bg)),
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
            .border_type(BorderType::Rounded)
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
        let total = rows.len();
        let height = inner.height as usize;
        self.scroll = self.scroll.min(total.saturating_sub(height) as u16);
        let scroll = self.scroll;
        frame.render_widget(Paragraph::new(rows).scroll((scroll, 0)), inner);
        super::scrollbar(frame, area, height, total, scroll as usize, theme);
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
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

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
        for _ in 0..4 {
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

    /// Every mode's hints render — the popup is derived from the same
    /// tables as dispatch, so coverage is the contract.
    #[test]
    fn every_mode_has_bindings() {
        for mode in MODES {
            assert!(
                !keymap::hints(mode).is_empty(),
                "{} must have bindings",
                mode.chip()
            );
        }
    }
}
