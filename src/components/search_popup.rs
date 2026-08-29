//! Search popup: VimInput on top, results list below (PLAN.md §1).
//! Enter submits the query and focuses results; Tab toggles focus back
//! (landing in INSERT); Esc: INSERT→NORMAL, NORMAL/results→close.
//! With results focused, `/` enters a local incremental filter over the
//! result set (SEARCH mode) — no network calls, same feel as pane filter.

use super::pane::{Entry, EntryKind, Pane};
use super::vim_input::{Outcome, SubMode, VimInput};
use crate::action::Action;
use crate::mode::Mode;
use crate::provider::SearchItem;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Input,
    Results,
}

pub struct SearchPopup {
    /// Active provider identity for the title (e.g. "search github").
    pub forge: String,
    pub input: VimInput,
    results: Pane,
    focus: Focus,
    /// `/` local filter over the results list.
    filter: VimInput,
    filtering: bool,
    /// Filter value before the current `/` session (Esc-cancel restore).
    pre_filter: String,
    /// A search request is in flight.
    pending: bool,
    /// Last search error, shown until the next submit.
    error: Option<String>,
    /// A query was submitted at least once (drives "no matches").
    submitted_once: bool,
}

impl Default for SearchPopup {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchPopup {
    pub fn new() -> Self {
        Self::with_prefill(None)
    }

    /// `prefill` seeds the query (resume flow: last repo from state).
    pub fn with_prefill(prefill: Option<&str>) -> Self {
        let mut results = Pane::new("results", vec![]);
        results.show_badges = true;
        let mut input = VimInput::new();
        if let Some(p) = prefill {
            // Replaceable: typing starts a fresh query; Enter resumes.
            input.prefill(p);
        }
        SearchPopup {
            forge: String::new(),
            input,
            results,
            focus: Focus::Input,
            filter: VimInput::transient(),
            filtering: false,
            pre_filter: String::new(),
            pending: false,
            error: None,
            submitted_once: false,
        }
    }

    /// Backend outcomes routed back into the popup.
    pub fn update(&mut self, action: &Action) {
        match action {
            Action::SearchSubmitted(_) => {
                self.submitted_once = true;
                self.pending = true;
                self.error = None;
                self.focus = Focus::Results;
            }
            Action::SearchResults { items } => {
                self.pending = false;
                let entries = items
                    .iter()
                    .map(|item| match item {
                        SearchItem::Repo(full) => Entry::new(full, EntryKind::Repo),
                        SearchItem::Org(login) => Entry::new(login, EntryKind::Org),
                    })
                    .collect();
                self.results = Pane::new("results", entries);
                self.results.show_badges = true;
            }
            Action::SearchFailed { error } => {
                self.pending = false;
                self.error = Some(crate::app::provider_status(error));
                self.results = Pane::new("results", vec![]);
                self.results.show_badges = true;
            }
            _ => {}
        }
    }

    /// Modeline chip while the popup is open.
    pub fn effective_mode(&self) -> Mode {
        if self.filtering {
            return Mode::Search;
        }
        match self.focus {
            Focus::Input => match self.input.submode {
                SubMode::Insert => Mode::Insert,
                SubMode::Normal => Mode::Normal,
            },
            Focus::Results => Mode::Browse,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Active `/` filter session captures everything until commit/cancel.
        if self.filtering {
            return match self.filter.handle_key(key) {
                Outcome::Changed => {
                    self.results.set_filter(self.filter.value());
                    Action::Noop
                }
                Outcome::Submitted => {
                    self.filtering = false; // commit: filter stays applied
                    Action::Noop
                }
                Outcome::Cancelled => {
                    self.results.set_filter(self.pre_filter.clone());
                    self.filtering = false;
                    Action::Noop
                }
                Outcome::Noop => Action::Noop,
            };
        }

        // Tab toggles focus; focusing the input always lands in INSERT.
        if key.code == KeyCode::Tab {
            self.toggle_focus();
            return Action::Noop;
        }

        match self.focus {
            Focus::Input => match self.input.handle_key(key) {
                Outcome::Submitted => {
                    self.filter.clear();
                    Action::SearchSubmitted(self.input.value())
                }
                Outcome::Cancelled => Action::ClosePopup,
                _ => Action::Noop,
            },
            Focus::Results => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.results.update(&Action::MoveDown);
                    Action::Noop
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.results.update(&Action::MoveUp);
                    Action::Noop
                }
                KeyCode::Char('/') => {
                    self.pre_filter = self.results.filter.clone();
                    self.filter.set(&self.pre_filter);
                    self.filtering = true;
                    Action::Noop
                }
                KeyCode::Char('y') => Action::SearchYank,
                KeyCode::Enter => {
                    if let Some(entry) = self.results.selected_entry() {
                        return match entry.kind {
                            EntryKind::Org => Action::OrgSelected(entry.name.clone()),
                            _ => {
                                let (owner, name) = split_repo(&entry.name);
                                Action::RepoSelected { owner, name }
                            }
                        };
                    }
                    Action::Noop
                }
                // Committed filter? First Esc clears it, second closes.
                KeyCode::Esc if !self.results.filter.is_empty() => {
                    self.results.set_filter(String::new());
                    Action::Noop
                }
                KeyCode::Esc => Action::ClosePopup,
                _ => Action::Noop,
            },
        }
    }

    /// The selected entry (for the yank action).
    pub fn selected_entry(&self) -> Option<&crate::components::pane::Entry> {
        self.results.selected_entry()
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Input => Focus::Results,
            Focus::Results => {
                self.input.submode = SubMode::Insert;
                Focus::Input
            }
        };
    }

    /// Cursor shape for the popup's text input (PLAN.md §5): bar in
    /// INSERT, block in NORMAL, hidden when results are focused.
    pub fn cursor_style(&self) -> Option<SetCursorStyle> {
        if self.focus != Focus::Input {
            return None;
        }
        Some(match self.input.submode {
            SubMode::Insert => SetCursorStyle::SteadyBar,
            SubMode::Normal => SetCursorStyle::SteadyBlock,
        })
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let popup = super::centered(area, 60, 60);

        // Clear first: reset cells beneath so nothing lingers (PLAN.md §9).
        frame.render_widget(Clear, popup);

        let hint = if self.filtering {
            " type to filter · enter commit · esc cancel "
        } else {
            " tab focus · enter submit/select · / filter · esc close "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                format!(" search {} ", self.forge),
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(hint, Style::default().fg(sem.hint)));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(inner);

        // Input field.
        let input_border = if self.focus == Focus::Input {
            sem.border_focused
        } else {
            sem.border_unfocused
        };
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(input_border))
            .style(Style::default().bg(sem.base));
        let input_inner = input_block.inner(rows[0]);
        frame.render_widget(input_block, rows[0]);
        let prompt = if self.focus == Focus::Input {
            Span::styled("❯ ", Style::default().fg(sem.border_focused))
        } else {
            Span::styled("❯ ", Style::default().fg(sem.overlay0))
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                prompt,
                Span::styled(self.input.value(), Style::default().fg(sem.text)),
            ])),
            input_inner,
        );

        // Cursor overlay while the input is focused (past the prompt).
        if self.focus == Focus::Input {
            let x = input_inner.x + 2 + self.input.cursor() as u16;
            if x < input_inner.x + input_inner.width {
                frame.set_cursor_position((x, input_inner.y));
            }
        }

        self.results.title = if self.pending {
            "results — searching…".into()
        } else if let Some(error) = &self.error {
            format!("results — error: {error}")
        } else if self.results.entries.is_empty() && self.submitted_once {
            "results — no matches".into()
        } else {
            "results".into()
        };
        self.results.focused = self.focus == Focus::Results;
        self.results.render(frame, rows[1], theme);
    }
}

fn split_repo(full: &str) -> (String, String) {
    let mut parts = full.splitn(2, '/');
    (
        parts.next().unwrap_or_default().to_string(),
        parts.next().unwrap_or_default().to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_shape_follows_input_submode() {
        let mut popup = SearchPopup::new();
        assert!(matches!(
            popup.cursor_style(),
            Some(SetCursorStyle::SteadyBar)
        ));

        popup.input.submode = SubMode::Normal;
        assert!(matches!(
            popup.cursor_style(),
            Some(SetCursorStyle::SteadyBlock)
        ));

        popup.toggle_focus(); // results focused → no text cursor
        assert!(popup.cursor_style().is_none());
    }

    #[test]
    fn prefill_seeds_query_and_replaces_on_typing() {
        use ratatui::crossterm::event::{
            KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
        };
        fn key(code: KeyCode) -> KeyEvent {
            KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            }
        }

        let mut popup = SearchPopup::with_prefill(Some("ratatui/ratatui"));
        assert_eq!(popup.input.value(), "ratatui/ratatui");

        // Typing replaces the prefill (cmdline semantics) — the resume
        // query must not concatenate behind a fresh query.
        popup.input.handle_key(key(KeyCode::Char('h')));
        popup.input.handle_key(key(KeyCode::Char('i')));
        assert_eq!(popup.input.value(), "hi");
    }
}
