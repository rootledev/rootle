//! Revision lenses: the blame + history state machines and the
//! history lens's rendering (moved from browser.rs, plans/0021
//! M2 — a pure move).

use super::{Browser, scrollbar};
use crate::components::vim_input::VimInput;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// The file-history lens over the preview pane (plans/0016 M1b).
/// Data arrives from the provider via `history_loaded`; until then the
/// lens shows a loading row.
pub struct History {
    /// The file the lens is open on.
    path: String,
    /// Cursor over the visible (filtered) rows.
    cursor: usize,
    scroll: u16,
    entries: Vec<crate::provider::LogEntry>,
    truncated: bool,
    loading: bool,
    /// Enter-from-blame: position the cursor at this sha on landing.
    pending_sha: Option<String>,
    /// Transient `/` session over the commits (house rule: every list
    /// filters) — subject, sha, and author match.
    filter: VimInput,
    filtering: bool,
    filter_value: String,
    pre_filter: String,
}

impl History {
    /// The lens positioned at a commit (blame → history composition).
    fn at(path: &str, pending_sha: Option<String>) -> Self {
        History {
            path: path.to_string(),
            cursor: 0,
            scroll: 0,
            entries: Vec::new(),
            truncated: false,
            loading: true,
            pending_sha,
            filter: VimInput::transient(),
            filtering: false,
            filter_value: String::new(),
            pre_filter: String::new(),
        }
    }

    fn loaded(&mut self, entries: Vec<crate::provider::LogEntry>, truncated: bool) {
        self.loading = false;
        self.truncated = truncated;
        self.entries = entries;
        // The blamed-commit composition lands the cursor on its row.
        if let Some(sha) = self.pending_sha.take()
            && let Some(i) = self
                .entries
                .iter()
                .position(|e| e.sha == sha || e.sha.starts_with(&sha))
        {
            self.cursor = self.visible().iter().position(|&vi| vi == i).unwrap_or(0);
        }
    }

    /// Entry indices surviving the committed filter.
    fn visible(&self) -> Vec<usize> {
        let needle = self.filter_value.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                needle.is_empty()
                    || c.subject.to_lowercase().contains(&needle)
                    || c.sha.contains(&needle)
                    || c.author.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn selected(&self) -> Option<&crate::provider::LogEntry> {
        let vis = self.visible();
        vis.get(self.cursor.min(vis.len().saturating_sub(1)))
            .map(|&i| &self.entries[i])
    }
}

/// Blame lens state (plans/0016 M1c): ranges for one path, fetched on
/// demand; the marks live in the Preview. Shared with the search
/// view's expanded pane (0019 parity).
pub(crate) struct BlameState {
    pub(crate) path: String,
    pub(crate) ranges: Vec<crate::provider::BlameRange>,
    pub(crate) loading: bool,
}

impl Browser {
    /// `␣ p b` off: drop the lens. (On is the app's: it fetches
    /// ranges via `blame_request` and they land in `blame_store`.)
    pub fn clear_blame(&mut self) {
        self.blame = None;
        self.preview.set_blame(None);
    }

    /// Toggle on: ranges already fetched for the file under preview
    /// apply immediately; otherwise the app spawns the fetch. Returns
    /// the (repo-path) the marks belong to when a fetch is needed.
    pub fn blame_toggle_on(&mut self) -> bool {
        if self.preview.text_line_count() == 0 {
            return false;
        }
        let apply = matches!(&self.blame, Some(b) if !b.loading);
        if apply {
            self.blame_apply();
        }
        true
    }

    /// The path a blame fetch should cover, if one is needed.
    pub fn blame_needed_for(&self) -> Option<String> {
        if self.preview.text_line_count() == 0 {
            return None;
        }
        match &self.blame {
            Some(b) if b.loading => None, // in flight
            Some(_) => None,              // loaded — blame_apply covers it
            None => self.selected_file().map(|(p, _)| p),
        }
    }

    pub fn blame_mark_loading(&mut self, path: String) {
        self.blame = Some(BlameState {
            path,
            ranges: Vec::new(),
            loading: true,
        });
    }

    /// Ranges landed (identity-checked by the caller); apply when the
    /// lens is open on this path.
    pub fn blame_store(&mut self, path: String, ranges: Vec<crate::provider::BlameRange>) {
        let active = self
            .blame
            .as_ref()
            .map(|b| b.loading && b.path == path)
            .unwrap_or(false);
        self.blame = Some(BlameState {
            path,
            ranges,
            loading: false,
        });
        if active {
            self.blame_apply();
        }
    }

    /// Ranges → per-line run marks on the preview (v1.5 shape:
    /// coalesced 1-based inclusive ranges; run starts carry the mark).
    fn blame_apply(&mut self) {
        let Some(b) = &self.blame else { return };
        let lines = self.preview.text_line_count();
        if lines == 0 {
            return;
        }
        let mut marks: Vec<Option<crate::components::preview::BlameMark>> = vec![None; lines];
        for r in &b.ranges {
            let start = (r.start_line as usize).saturating_sub(1);
            if start < lines {
                marks[start] = Some(crate::components::preview::BlameMark {
                    sha: r.sha.chars().take(7).collect(),
                    author: r.author.clone(),
                });
            }
        }
        self.preview.set_blame(Some(marks));
    }

    /// Enter on a blame line: the sha the margin names for that line —
    /// the history lens opens positioned at it. None outside blame.
    pub fn blame_line_sha(&self) -> Option<String> {
        if !self.preview.blaming() {
            return None;
        }
        let b = self.blame.as_ref()?;
        if b.loading {
            return None;
        }
        let line = self.preview_line().unwrap_or(1) as usize;
        b.ranges
            .iter()
            .find(|r| r.start_line as usize <= line && line <= r.end_line as usize)
            .map(|r| r.sha.clone())
    }

    /// `␣ p h` opens the history lens on the previewed file, if any —
    /// a loading row first; entries land via `history_loaded`.
    /// `at_sha` positions the cursor (blame → history composition).
    pub fn open_history(&mut self, at_sha: Option<String>) -> bool {
        match self.selected_file() {
            Some((path, _)) => {
                self.history = Some(History::at(&path, at_sha));
                true
            }
            None => false,
        }
    }

    /// Log entries landed from the provider.
    pub fn history_loaded(&mut self, entries: Vec<crate::provider::LogEntry>, truncated: bool) {
        if let Some(h) = &mut self.history {
            h.loaded(entries, truncated);
        }
    }

    /// The path the open lens serves (event identity check).
    pub fn history_path(&self) -> Option<&str> {
        self.history.as_ref().map(|h| h.path.as_str())
    }

    pub fn history_active(&self) -> bool {
        self.history.is_some()
    }

    /// History lens `/` session active (keys route to the filter).
    pub fn history_filtering(&self) -> bool {
        self.history.as_ref().map(|h| h.filtering).unwrap_or(false)
    }

    /// Begin the `/` session (remembers the committed filter so the
    /// session's Esc restores it).
    pub fn history_begin_filter(&mut self) {
        if let Some(h) = &mut self.history {
            h.pre_filter = h.filter_value.clone();
            h.filtering = true;
        }
    }

    /// A key for the filter session; commits/cancels end it. The list
    /// narrows as you type (pane-style live filter, not commit-only).
    pub fn history_filter_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        if let Some(h) = &mut self.history {
            match h.filter.handle_key(key) {
                crate::components::vim_input::Outcome::Changed => {
                    h.filter_value = h.filter.value();
                    h.cursor = 0;
                    h.scroll = 0;
                }
                crate::components::vim_input::Outcome::Submitted => {
                    h.filtering = false;
                    h.filter_value = h.filter.value();
                    h.cursor = 0;
                    h.scroll = 0;
                }
                crate::components::vim_input::Outcome::Cancelled => {
                    h.filtering = false;
                    h.filter_value = h.pre_filter.clone();
                }
                _ => {}
            }
        }
    }

    /// Esc in the lens: a committed filter clears first (the wizard
    /// ladder), the second Esc closes. Returns true when it closed.
    pub fn history_esc(&mut self) -> bool {
        if let Some(h) = &mut self.history
            && !h.filter_value.is_empty()
        {
            h.filter_value.clear();
            h.cursor = 0;
            return false;
        }
        self.close_history();
        true
    }

    pub fn history_move(&mut self, delta: i64) {
        if let Some(h) = &mut self.history {
            let len = h.visible().len() as i64;
            if len > 0 {
                h.cursor = ((h.cursor as i64 + delta).rem_euclid(len)) as usize;
            }
        }
    }

    pub fn at_commit_view(&self) -> bool {
        self.at_commit.is_some()
    }

    /// The picked commit as a LogEntry (the band needs the subject).
    pub fn history_pick_entry(&self) -> Option<crate::provider::LogEntry> {
        self.history.as_ref()?.selected().cloned()
    }

    /// The picked commit (path + full sha) — open-at-commit and the
    /// permalink yank.
    pub fn history_pick(&self) -> Option<(String, String)> {
        let h = self.history.as_ref()?;
        let c = h.selected()?;
        Some((h.path.clone(), c.sha.clone()))
    }

    /// tig-shaped commit list for the previewed file (mock data).
    pub(crate) fn render_history(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let Some(h) = &mut self.history else {
            return;
        };
        let sem = &theme.semantic;
        let mut title = format!(" history — {} ", h.path);
        if h.filtering || !h.filter_value.is_empty() {
            title = format!(" history — {} /{} ", h.path, h.filter.value());
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(title, Style::default().fg(sem.subtext0)))
            .title_bottom(Span::styled(
                " j/k commit · enter file at commit · / filter · esc back ",
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if h.loading {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  loading history…",
                    Style::default().fg(sem.subtext0),
                ))),
                inner,
            );
            return;
        }
        let vis = h.visible();
        let mut lines: Vec<Line> = Vec::new();
        for (row, &i) in vis.iter().enumerate() {
            let c = &h.entries[i];
            let selected = row == h.cursor;
            let style = if selected {
                Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
            } else {
                Style::default().fg(sem.text)
            };
            let dim = if selected {
                style
            } else {
                Style::default().fg(sem.subtext0)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "▌ " } else { "  " },
                    Style::default().fg(sem.border_focused),
                ),
                Span::styled(
                    format!("{} ", c.sha.chars().take(7).collect::<String>()),
                    Style::default().fg(sem.warning),
                ),
                Span::styled(c.subject.clone(), style),
            ]));
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(format!("{} · {}", c.author, c.date), dim),
            ]));
        }
        if vis.is_empty() {
            lines.push(Line::from(Span::styled(
                "  no matching commits",
                Style::default().fg(sem.subtext0),
            )));
        }
        // Bounded compute: past the render budget the provider said
        // truncated — narrow with /.
        if h.truncated {
            lines.push(Line::from(Span::styled(
                "  ⋮ truncated — / filters",
                Style::default().fg(sem.subtext0),
            )));
        }
        let height = inner.height as usize;
        let total = lines.len();
        let max_scroll = total.saturating_sub(height) as u16;
        // Keep the selected commit's two-line row visible.
        let want = (h.cursor * 2) as u16;
        if want < h.scroll {
            h.scroll = want;
        } else if want + 1 >= h.scroll + height as u16 {
            h.scroll = (want + 2).saturating_sub(height as u16);
        }
        h.scroll = h.scroll.min(max_scroll);
        let scroll = h.scroll;
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
        scrollbar(frame, area, height, total, scroll as usize, theme);
    }
}
