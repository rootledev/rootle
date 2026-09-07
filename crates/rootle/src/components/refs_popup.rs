//! Branch/tag radio list. Selection previews the ref; Enter commits it.
//! The shared list filter handles live narrowing and the Esc restore ladder.

use super::list_view::{
    Boundary, FilterOutcome, ItemIndex, ListCursor, ListFilter, ListMovement, RowSpan, Viewport,
};
use crate::action::Action;
use crate::components::centered_clamped;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear};

/// One row: a branch or tag from the provider (v1.5 `repo/refs`).
#[derive(Debug, Clone)]
struct Row {
    name: String,
    sha: String,
    is_tag: bool,
    is_default: bool,
}

pub struct RefsPopup {
    selection: ListCursor,
    viewport: Viewport,
    /// Committed-at-open revision — Esc reverts the live preview.
    baseline: String,
    /// None until the provider answers (the popup shows a loading row).
    rows: Option<Vec<Row>>,
    filter: ListFilter,
}

impl RefsPopup {
    pub(crate) fn diagnostics(&self, full: bool) -> serde_json::Value {
        serde_json::json!({"selected":self.selection.selected().get(),
            "loaded":self.rows.is_some(), "entries":self.rows.as_ref().map(Vec::len),
            "viewport":self.viewport.diagnostics(), "filter":self.filter.diagnostics(full)})
    }

    pub fn effective_mode(&self) -> crate::mode::Mode {
        if self.filter.active() {
            crate::mode::Mode::Search
        } else {
            crate::mode::Mode::Browse
        }
    }
    pub fn new(current: &str) -> Self {
        RefsPopup {
            selection: ListCursor::new(),
            viewport: Viewport::default(),
            baseline: current.to_string(),
            rows: None,
            filter: ListFilter::default(),
        }
    }

    /// The provider's refs landed — the loading row becomes the list,
    /// cursor on the current revision when it's in it.
    pub fn set_refs(&mut self, refs: rootle_provider::RepoRefs) {
        let selected = self
            .selected()
            .map(|row| row.name.clone())
            .unwrap_or_else(|| self.baseline.clone());
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
        self.rows = Some(rows);
        let visible = self.visible();
        let selected = visible
            .iter()
            .position(|&index| {
                self.rows
                    .as_ref()
                    .is_some_and(|rows| rows[index].name == selected)
            })
            .unwrap_or(0);
        self.selection.select(ItemIndex::new(selected));
    }

    pub fn baseline(&self) -> &str {
        &self.baseline
    }

    /// Row indices surviving the committed filter, branches then tags.
    fn visible(&self) -> Vec<usize> {
        let Some(rows) = &self.rows else {
            return vec![];
        };
        self.filter
            .visible(rows, |row, filter| filter.matches(&row.name))
    }

    fn selected(&self) -> Option<&Row> {
        let rows = self.rows.as_ref()?;
        let vis = self.visible();
        vis.get(
            self.selection
                .selected()
                .get()
                .min(vis.len().saturating_sub(1)),
        )
        .map(|&i| &rows[i])
    }

    pub fn handle_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> Action {
        use crate::keymap::{ListCommand, ListContext, list_command};
        if self.filter.active() {
            if self.filter.handle_key(key) != FilterOutcome::Unchanged {
                self.selection.reset();
                self.viewport.reset();
                return self
                    .selected()
                    .map(|row| Action::RefsPreview(row.name.clone()))
                    .unwrap_or(Action::Noop);
            }
            return Action::Noop;
        }
        match list_command(ListContext::Refs, key) {
            Some(ListCommand::Cancel) if self.filter.clear() => {
                self.selection.reset();
                self.viewport.reset();
                self.selected()
                    .map(|row| Action::RefsPreview(row.name.clone()))
                    .unwrap_or(Action::Noop)
            }
            Some(ListCommand::Cancel) => Action::ClosePopup,
            Some(ListCommand::Filter) => {
                self.filter.begin();
                Action::Noop
            }
            Some(ListCommand::Next) => self.step(ListMovement::Next),
            Some(ListCommand::Previous) => self.step(ListMovement::Previous),
            Some(ListCommand::First) => self.step(ListMovement::First),
            Some(ListCommand::Last) => self.step(ListMovement::Last),
            Some(ListCommand::Accept) => self
                .selected()
                .map(|r| Action::RefsCommit(r.name.to_string()))
                .unwrap_or(Action::Noop),
            _ => Action::Noop,
        }
    }

    fn step(&mut self, movement: ListMovement) -> Action {
        self.selection
            .advance(movement, self.visible().len(), Boundary::Wrap);
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
        if self.filter.active() || !self.filter.is_empty() {
            title = format!(" revisions — /{} ", self.filter.text());
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
                crate::keymap::hint_row(crate::keymap::list_hints(
                    crate::keymap::ListContext::Refs,
                    self.filter.active(),
                )),
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let vis = self.visible();
        let mut lines: Vec<Line> = Vec::new();
        let mut seen_tags = false;
        let mut selected_row = None;
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
            let cursor = row == self.selection.selected().get();
            if cursor {
                selected_row = Some(RowSpan::single(lines.len()));
            }
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
        self.viewport
            .render(frame, popup, inner, lines, selected_row, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use rootle_provider::{RefInfo, RepoRefs};

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
