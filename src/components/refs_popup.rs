//! Revision switcher (plans/0016 M1a, mock stage): branches and tags
//! as a radio list — the scope-popup pattern. Moving the cursor
//! live-previews the switch in the modeline crumb, Enter commits,
//! Esc reverts; `/` filters (house rule: every list filters).
//!
//! MOCK: the refs are baked in — nothing is fetched and switching
//! repaints the crumb only. The wire shape (`repo/refs` + `ref?` on
//! `repo/tree`) lands after the UX is reviewed.

use super::vim_input::{Outcome, VimInput};
use crate::action::Action;
use crate::components::{centered, scrollbar};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// One revision row: branch or tag.
#[derive(Debug, Clone)]
struct MockRef {
    name: &'static str,
    sha: &'static str,
    is_tag: bool,
    is_default: bool,
}

/// Deterministic stand-ins — what `repo/refs` will return.
const MOCK_REFS: &[MockRef] = &[
    MockRef {
        name: "main",
        sha: "a1b2c3d",
        is_tag: false,
        is_default: true,
    },
    MockRef {
        name: "release/2.7",
        sha: "e4f5a6b",
        is_tag: false,
        is_default: false,
    },
    MockRef {
        name: "feature/miller-panes",
        sha: "c7d8e9f",
        is_tag: false,
        is_default: false,
    },
    MockRef {
        name: "fix/cache-eviction",
        sha: "0a1b2c3",
        is_tag: false,
        is_default: false,
    },
    MockRef {
        name: "v0.7.1",
        sha: "d4e5f6a",
        is_tag: true,
        is_default: false,
    },
    MockRef {
        name: "v0.7.0",
        sha: "7b8c9d0",
        is_tag: true,
        is_default: false,
    },
    MockRef {
        name: "v0.6.0",
        sha: "1e2f3a4",
        is_tag: true,
        is_default: false,
    },
];

pub struct RefsPopup {
    cursor: usize,
    /// Committed-at-open revision — Esc reverts the live preview.
    baseline: String,
    filter: VimInput,
    filtering: bool,
    filter_value: String,
    /// Committed filter before the live session — Esc restores it.
    pre_filter: String,
}

impl RefsPopup {
    pub fn new(current: &str) -> Self {
        let cursor = MOCK_REFS
            .iter()
            .position(|r| r.name == current)
            .unwrap_or(0);
        RefsPopup {
            cursor,
            baseline: current.to_string(),
            filter: VimInput::transient(),
            filtering: false,
            filter_value: String::new(),
            pre_filter: String::new(),
        }
    }

    pub fn baseline(&self) -> &str {
        &self.baseline
    }

    /// Row indices surviving the committed filter, branches then tags.
    fn visible(&self) -> Vec<usize> {
        let needle = self.filter_value.to_lowercase();
        MOCK_REFS
            .iter()
            .enumerate()
            .filter(|(_, r)| needle.is_empty() || r.name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    fn selected(&self) -> Option<&'static MockRef> {
        let vis = self.visible();
        vis.get(self.cursor.min(vis.len().saturating_sub(1)))
            .map(|&i| &MOCK_REFS[i])
    }

    pub fn handle_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> Action {
        use ratatui::crossterm::event::KeyCode;
        if self.filtering {
            return match self.filter.handle_key(key) {
                Outcome::Changed => {
                    // Pane-style live filter: the list narrows per
                    // keystroke and the live crumb preview follows.
                    self.filter_value = self.filter.value();
                    self.cursor = 0;
                    self.selected()
                        .map(|r| Action::RefsPreview(r.name.to_string()))
                        .unwrap_or(Action::Noop)
                }
                Outcome::Submitted => {
                    self.filtering = false;
                    self.cursor = 0;
                    self.selected()
                        .map(|r| Action::RefsPreview(r.name.to_string()))
                        .unwrap_or(Action::Noop)
                }
                Outcome::Cancelled => {
                    self.filtering = false;
                    self.filter_value = self.pre_filter.clone();
                    Action::Noop
                }
                Outcome::Noop => Action::Noop,
            };
        }
        match key.code {
            KeyCode::Esc => Action::ClosePopup,
            KeyCode::Char('/') => {
                self.pre_filter = self.filter_value.clone();
                self.filtering = true;
                Action::Noop
            }
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::Enter => self
                .selected()
                .map(|r| Action::RefsCommit(r.name.to_string()))
                .unwrap_or(Action::Noop),
            _ => Action::Noop,
        }
    }

    fn step(&mut self, delta: i64) -> Action {
        let len = self.visible().len() as i64;
        if len == 0 {
            return Action::Noop;
        }
        self.cursor = ((self.cursor as i64 + delta).rem_euclid(len)) as usize;
        self.selected()
            .map(|r| Action::RefsPreview(r.name.to_string()))
            .unwrap_or(Action::Noop)
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let popup = centered(area, 50, 50);
        frame.render_widget(Clear, popup);

        let current = self.selected().map(|r| r.name).unwrap_or("");
        let mut title = format!(" revisions — @ {current} ");
        if self.filtering || !self.filter_value.is_empty() {
            title = format!(" revisions — /{} ", self.filter.value());
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                title,
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                " j/k move · enter switch · / filter · esc revert ",
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let vis = self.visible();
        let mut lines: Vec<Line> = Vec::new();
        let mut seen_tags = false;
        for (row, &i) in vis.iter().enumerate() {
            let r = &MOCK_REFS[i];
            if r.is_tag && !seen_tags {
                seen_tags = true;
                lines.push(Line::from(Span::styled(
                    "  tags",
                    Style::default().fg(sem.hint),
                )));
            }
            let cursor = row == self.cursor;
            let radio = if r.name == self.baseline {
                "(•)"
            } else {
                "( )"
            };
            let kind = if r.is_tag { "tag " } else { "" };
            let default = if r.is_default { " · default" } else { "" };
            let fg = if cursor { sem.selection_fg } else { sem.text };
            let mut style = Style::default().fg(fg);
            if cursor {
                style = style.bg(sem.selection_bg);
            }
            let dim = Style::default().fg(sem.subtext0);
            lines.push(Line::from(vec![
                Span::styled(format!("{radio} "), Style::default().fg(sem.border_focused)),
                Span::styled(r.name.to_string(), style),
                Span::styled(format!("  {kind}{}{default}", r.sha), dim),
            ]));
        }
        if vis.is_empty() {
            lines.push(Line::from(Span::styled(
                "  no matching revisions",
                Style::default().fg(sem.subtext0),
            )));
        }
        let total = lines.len();
        let height = inner.height as usize;
        let scroll = self
            .cursor
            .saturating_sub(height.saturating_sub(1))
            .min(total.saturating_sub(height));
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
        scrollbar(frame, popup, height, total, scroll, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn cursor_previews_and_enter_commits() {
        let mut p = RefsPopup::new("main");
        assert_eq!(
            p.handle_key(key(KeyCode::Char('j'))),
            Action::RefsPreview("release/2.7".into())
        );
        assert_eq!(
            p.handle_key(key(KeyCode::Char('j'))),
            Action::RefsPreview("feature/miller-panes".into())
        );
        assert_eq!(
            p.handle_key(key(KeyCode::Enter)),
            Action::RefsCommit("feature/miller-panes".into())
        );
        // Wraps around the end.
        let mut p = RefsPopup::new("main");
        assert_eq!(
            p.handle_key(key(KeyCode::Char('k'))),
            Action::RefsPreview("v0.6.0".into())
        );
    }

    #[test]
    fn slash_filter_narrows_and_esc_cancels() {
        let mut p = RefsPopup::new("main");
        p.handle_key(key(KeyCode::Char('/')));
        for c in "release".chars() {
            p.handle_key(key(KeyCode::Char(c)));
        }
        let action = p.handle_key(key(KeyCode::Enter)); // commit filter
        assert_eq!(action, Action::RefsPreview("release/2.7".into()));
        assert_eq!(
            p.handle_key(key(KeyCode::Enter)),
            Action::RefsCommit("release/2.7".into())
        );
    }
}
