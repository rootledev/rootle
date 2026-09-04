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
use crate::components::{centered_clamped, scrollbar};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// One row: a branch or tag from the provider (v1.5 `repo/refs`).
#[derive(Debug, Clone)]
struct Row {
    name: String,
    sha: String,
    is_tag: bool,
    is_default: bool,
}

pub struct RefsPopup {
    cursor: usize,
    /// Committed-at-open revision — Esc reverts the live preview.
    baseline: String,
    /// None until the provider answers (the popup shows a loading row).
    rows: Option<Vec<Row>>,
    filter: VimInput,
    filtering: bool,
    filter_value: String,
    /// Committed filter before the live session — Esc restores it.
    pre_filter: String,
}

impl RefsPopup {
    pub fn new(current: &str) -> Self {
        RefsPopup {
            cursor: 0,
            baseline: current.to_string(),
            rows: None,
            filter: VimInput::transient(),
            filtering: false,
            filter_value: String::new(),
            pre_filter: String::new(),
        }
    }

    /// The provider's refs landed — the loading row becomes the list,
    /// cursor on the current revision when it's in it.
    pub fn set_refs(&mut self, refs: crate::provider::RepoRefs) {
        let mut rows: Vec<Row> = refs
            .branches
            .into_iter()
            .map(|r| Row {
                name: r.name,
                sha: r.sha,
                is_tag: false,
                is_default: r.is_default,
            })
            .collect();
        rows.extend(refs.tags.into_iter().map(|r| Row {
            name: r.name,
            sha: r.sha,
            is_tag: true,
            is_default: false,
        }));
        self.cursor = rows
            .iter()
            .position(|r| r.name == self.baseline)
            .unwrap_or(0);
        self.rows = Some(rows);
    }

    pub fn baseline(&self) -> &str {
        &self.baseline
    }

    /// Row indices surviving the committed filter, branches then tags.
    fn visible(&self) -> Vec<usize> {
        let Some(rows) = &self.rows else {
            return vec![];
        };
        let needle = self.filter_value.to_lowercase();
        rows.iter()
            .enumerate()
            .filter(|(_, r)| needle.is_empty() || r.name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    fn selected(&self) -> Option<&Row> {
        let rows = self.rows.as_ref()?;
        let vis = self.visible();
        vis.get(self.cursor.min(vis.len().saturating_sub(1)))
            .map(|&i| &rows[i])
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
        let popup = centered_clamped(area, 50, 50, 30, 10);
        frame.render_widget(Clear, popup);

        let current = self.selected().map(|r| r.name.as_str()).unwrap_or("");
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
            let r = &self
                .rows
                .as_ref()
                .expect("visible() is empty while loading")[i];
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
            let short: String = r.sha.chars().take(7).collect();
            lines.push(Line::from(vec![
                Span::styled(format!("{radio} "), Style::default().fg(sem.border_focused)),
                Span::styled(r.name.clone(), style),
                Span::styled(format!("  {kind}{short}{default}"), dim),
            ]));
        }
        if self.rows.is_none() {
            lines.push(Line::from(Span::styled(
                "  loading revisions…",
                Style::default().fg(sem.subtext0),
            )));
        } else if vis.is_empty() {
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
    use crate::provider::{RefInfo, RepoRefs};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn loaded_popup(current: &str) -> RefsPopup {
        let mut p = RefsPopup::new(current);
        p.set_refs(RepoRefs {
            branches: vec![
                RefInfo {
                    name: "main".into(),
                    sha: "a1b2c3d".into(),
                    is_default: true,
                },
                RefInfo {
                    name: "release/2.7".into(),
                    sha: "e4f5a6b".into(),
                    is_default: false,
                },
                RefInfo {
                    name: "feature/miller-panes".into(),
                    sha: "c7d8e9f".into(),
                    is_default: false,
                },
            ],
            tags: vec![RefInfo {
                name: "v0.7.1".into(),
                sha: "d4e5f6a".into(),
                is_default: false,
            }],
        });
        p
    }

    #[test]
    fn cursor_previews_and_enter_commits() {
        let mut p = loaded_popup("main");
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
        // Wraps around the end (into the tags).
        let mut p = loaded_popup("main");
        assert_eq!(
            p.handle_key(key(KeyCode::Char('k'))),
            Action::RefsPreview("v0.7.1".into())
        );
    }

    #[test]
    fn slash_filter_narrows_and_esc_cancels() {
        let mut p = loaded_popup("main");
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

    #[test]
    fn loading_state_offers_nothing() {
        let mut p = RefsPopup::new("main");
        assert_eq!(p.handle_key(key(KeyCode::Enter)), Action::Noop);
        assert_eq!(p.handle_key(key(KeyCode::Char('j'))), Action::Noop);
    }
}
