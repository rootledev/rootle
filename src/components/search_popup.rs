//! Search popup: VimInput on top, results list below (PLAN.md §1).
//! Enter submits the query and focuses results; Tab toggles focus back
//! (landing in INSERT); Esc: INSERT→NORMAL, NORMAL/results→close.
//! With results focused, `/` enters a local incremental filter over the
//! result set (SEARCH mode) — no network calls, same feel as pane filter.

use super::pane::{Entry, EntryKind, Pane};
use super::vim_input::{Outcome, SubMode, VimInput};
use crate::action::Action;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Input,
    Results,
}

pub struct SearchPopup {
    pub input: VimInput,
    results: Pane,
    focus: Focus,
    /// `/` local filter over the results list.
    filter: VimInput,
    filtering: bool,
    /// Filter value before the current `/` session (Esc-cancel restore).
    pre_filter: String,
}

impl SearchPopup {
    pub fn new() -> Self {
        let results = Pane::new("results", mock_search(""));
        SearchPopup {
            input: VimInput::new(),
            results,
            focus: Focus::Input,
            filter: VimInput::transient(),
            filtering: false,
            pre_filter: String::new(),
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
                    self.results = Pane::new("results", mock_search(&self.input.value()));
                    self.filter.clear();
                    self.focus = Focus::Results;
                    Action::Noop
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
                KeyCode::Enter => {
                    if let Some(entry) = self.results.selected_entry() {
                        let (owner, name) = split_repo(&entry.name);
                        return Action::RepoSelected { owner, name };
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

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Input => Focus::Results,
            Focus::Results => {
                self.input.submode = SubMode::Insert;
                Focus::Input
            }
        };
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let popup = centered(area, 60, 60);

        // Clear first: reset cells beneath so nothing lingers (PLAN.md §9).
        frame.render_widget(Clear, popup);

        let hint = if self.filtering {
            " type to filter · enter commit · esc cancel "
        } else {
            " tab focus · enter submit/select · / filter · esc close "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                " search github ",
                Style::default()
                    .fg(sem.text)
                    .add_modifier(Modifier::BOLD),
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
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(input_border))
            .style(Style::default().bg(sem.base));
        let input_inner = input_block.inner(rows[0]);
        frame.render_widget(input_block, rows[0]);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                self.input.value(),
                Style::default().fg(sem.text),
            ))),
            input_inner,
        );

        // Cursor overlay while the input is focused.
        if self.focus == Focus::Input {
            let x = input_inner.x + self.input.cursor() as u16;
            if x < input_inner.x + input_inner.width {
                frame.set_cursor_position((x, input_inner.y));
            }
        }

        self.results.focused = self.focus == Focus::Results;
        self.results.render(frame, rows[1], theme);
    }
}

fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
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

/// Mock search — replaced by the GitHub search endpoint (milestone 3).
fn mock_search(query: &str) -> Vec<Entry> {
    let all = [
        "ratatui/ratatui",
        "ratatui/templates",
        "tokio-rs/tokio",
        "tokio-rs/axum",
        "helix-editor/helix",
        "sharkdp/bat",
        "BurntSushi/ripgrep",
    ];
    let q = query.to_lowercase();
    all.iter()
        .filter(|r| q.is_empty() || r.to_lowercase().contains(&q))
        .map(|r| Entry::new(r, EntryKind::Repo))
        .collect()
}

fn split_repo(full: &str) -> (String, String) {
    let mut parts = full.splitn(2, '/');
    (
        parts.next().unwrap_or_default().to_string(),
        parts.next().unwrap_or_default().to_string(),
    )
}
