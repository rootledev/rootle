//! Keybinds popup (`?`, plans/0003 §2): every binding, derived from
//! the keymap tables — the same source of truth as dispatch, so this
//! list can never drift. j/k scroll, Esc closes. Section headers are
//! the modes' own chips; the header row carries the app version.

use super::modeline::mode_color;
use crate::action::Action;
use crate::keymap;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct KeybindsPopup {
    scroll: u16,
}

impl KeybindsPopup {
    pub fn new() -> Self {
        KeybindsPopup { scroll: 0 }
    }

    /// Sectioned, fully themed rows (built with the palette, not
    /// re-styled after the fact).
    fn rows(&self, theme: &Theme) -> Vec<Line<'static>> {
        let sem = &theme.semantic;
        const MODES: [Mode; 6] = [
            Mode::Browse,
            Mode::Search,
            Mode::Insert,
            Mode::Normal,
            Mode::Leader,
            Mode::Visual,
        ];

        let mut rows = vec![
            Line::from(vec![
                Span::styled(
                    " ghx ",
                    Style::default()
                        .fg(sem.crust)
                        .bg(sem.border_focused)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" v{VERSION} · a github browser "),
                    Style::default().fg(sem.subtext0),
                ),
            ]),
            Line::raw(""),
        ];
        for mode in MODES {
            // The mode's own chip as the section header.
            rows.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!(" {} ", mode.chip()),
                    Style::default()
                        .fg(sem.crust)
                        .bg(mode_color(mode, sem))
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            for (key, desc) in keymap::hints(mode) {
                rows.push(Line::from(vec![
                    Span::styled(
                        format!("      {key:8}"),
                        Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(desc.to_string(), Style::default().fg(sem.subtext0)),
                ]));
            }
            rows.push(Line::raw(""));
        }
        rows
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
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
        let popup = super::centered(area, 50, 70);
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
            .title_bottom(Span::styled(
                " j/k scroll · esc close ",
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let rows = self.rows(theme);
        let max_scroll = rows.len().saturating_sub(inner.height as usize) as u16;
        self.scroll = self.scroll.min(max_scroll);
        frame.render_widget(Paragraph::new(rows).scroll((self.scroll, 0)), inner);
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
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn rows_cover_every_mode_and_the_version() {
        let popup = KeybindsPopup::new();
        let rows = popup.rows(&Theme::catppuccin_mocha());
        let text: String = rows
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        for chip in ["BROWSE", "SEARCH", "INSERT", "NORMAL", "LEADER", "VISUAL"] {
            assert!(text.contains(chip), "missing section {chip}");
        }
        assert!(text.contains(VERSION), "version header missing");
        assert!(text.contains("find file")); // leader entries present
    }

    #[test]
    fn esc_closes_and_jk_scroll() {
        let mut popup = KeybindsPopup::new();
        popup.handle_key(key(KeyCode::Char('j')));
        popup.handle_key(key(KeyCode::Char('j')));
        assert_eq!(popup.scroll, 2);
        popup.handle_key(key(KeyCode::Char('k')));
        assert_eq!(popup.scroll, 1);
        assert_eq!(popup.handle_key(key(KeyCode::Esc)), Action::ClosePopup);
    }
}
