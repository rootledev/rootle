//! Repository/file history, fully identified requests and shared list rendering.

use super::Browser;
use crate::components::list_view::{
    Boundary, FilterOutcome, ItemIndex, ListCursor, ListFilter, ListMovement, RowSpan, Viewport,
};
use crate::request::{HistoryRequest, HistoryScope};
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use rootle_provider::{GitRef, RepoId, Sha};

/// Repository or file history at the browsed revision.
/// Data arrives from the provider via `history_loaded`; until then the
/// lens shows a loading row.
pub struct History {
    request: HistoryRequest,
    /// Cursor over the visible (filtered) rows.
    selection: ListCursor,
    viewport: Viewport,
    entries: Vec<HistoryEntry>,
    truncated: bool,
    loading: bool,
    /// Enter-from-blame: position the cursor at this sha on landing.
    pending_sha: Option<Sha>,
    failure: Option<String>,
    /// Transient `/` session over the commits (house rule: every list
    /// filters) — subject, sha, and author match.
    filter: ListFilter,
}

#[derive(Clone)]
pub struct HistoryEntry {
    pub revision: Sha,
    pub subject: String,
    pub author: String,
    pub date: String,
}

impl History {
    /// The lens positioned at a commit (blame → history composition).
    fn at(request: HistoryRequest, pending_sha: Option<Sha>) -> Self {
        History {
            request,
            selection: ListCursor::new(),
            viewport: Viewport::default(),
            entries: Vec::new(),
            truncated: false,
            loading: true,
            pending_sha,
            failure: None,
            filter: ListFilter::default(),
        }
    }

    fn loaded(&mut self, entries: Vec<rootle_provider::LogEntry>, truncated: bool) {
        let previous = self.selected().map(|entry| entry.revision.clone());
        self.loading = false;
        self.truncated = truncated;
        self.failure = None;
        self.entries = entries
            .into_iter()
            .map(|entry| HistoryEntry {
                revision: entry.sha.into(),
                subject: crate::sanitize::sanitize_inline(&entry.subject),
                author: crate::sanitize::sanitize_inline(&entry.author),
                date: crate::sanitize::sanitize_inline(&entry.date),
            })
            .collect();
        // The blamed-commit composition lands the cursor on its row.
        if let Some(sha) = self.pending_sha.take().or(previous)
            && let Some(i) = self.entries.iter().position(|entry| entry.revision == sha)
        {
            self.selection.select(ItemIndex::new(
                self.visible()
                    .iter()
                    .position(|&index| index == i)
                    .unwrap_or(0),
            ));
        }
        self.selection.clamp(self.visible().len());
    }

    /// Entry indices surviving the committed filter.
    fn visible(&self) -> Vec<usize> {
        self.filter.visible(&self.entries, |entry, filter| {
            filter.matches(&entry.subject)
                || filter.matches(entry.revision.as_str())
                || filter.matches(&entry.author)
        })
    }

    fn selected(&self) -> Option<&HistoryEntry> {
        let vis = self.visible();
        vis.get(
            self.selection
                .selected()
                .get()
                .min(vis.len().saturating_sub(1)),
        )
        .map(|&i| &self.entries[i])
    }

    /// Counts/cursor summary for session traces (plans/0030).
    pub(crate) fn diagnostics(&self) -> HistoryDiagnostics {
        HistoryDiagnostics {
            entries: self.entries.len(),
            visible: self.visible().len(),
            selected: self.selection.selected().get(),
            loading: self.loading,
            truncated: self.truncated,
        }
    }
}

/// Diagnostic summary of the history lens (plans/0030 session
/// traces): counts and cursor only, no commit text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HistoryDiagnostics {
    pub(crate) entries: usize,
    pub(crate) visible: usize,
    pub(crate) selected: usize,
    pub(crate) loading: bool,
    pub(crate) truncated: bool,
}
impl Browser {
    /// File history keeps its path identity even while inspecting a commit.
    pub fn open_history(&mut self, at_sha: Option<String>) -> bool {
        let Some((path, _)) = self.selected_file() else {
            return false;
        };
        self.begin_history(HistoryScope::File(path.into()), at_sha.map(Sha::from))
    }

    pub fn open_repository_history(&mut self) -> bool {
        self.begin_history(HistoryScope::Repository, None)
    }

    fn begin_history(&mut self, scope: HistoryScope, pending_sha: Option<Sha>) -> bool {
        let Some(repository) = self.history_repository() else {
            return false;
        };
        let request = HistoryRequest {
            repository,
            revision: self.current_ref().map(GitRef::from),
            scope,
            generation: self.history_generation.tick(),
        };
        self.history = Some(History::at(request, pending_sha));
        true
    }

    /// The selected repository, including the repos pane before a tree loads.
    fn history_repository(&self) -> Option<RepoId> {
        let repositories = self.levels.get(1)?;
        let selected = repositories.selected_entry()?;
        if selected.kind != super::EntryKind::Repo || self.focus == 0 {
            return None;
        }
        let organization = self.selected_org()?;
        Some(RepoId::from(format!("{organization}/{}", selected.name)))
    }

    pub fn history_request(&self) -> Option<&HistoryRequest> {
        self.history.as_ref().map(|history| &history.request)
    }

    pub fn history_accepts(&self, request: &HistoryRequest) -> bool {
        self.history_request() == Some(request)
            && self.history_repository().as_ref() == Some(&request.repository)
            && self.current_ref() == request.revision.as_ref().map(GitRef::as_str)
    }

    pub fn repository_history_active(&self) -> bool {
        self.history_request()
            .is_some_and(|request| request.scope == HistoryScope::Repository)
    }

    pub fn history_failed(&mut self, message: &str) {
        if let Some(history) = &mut self.history {
            history.loading = false;
            history.failure = Some(crate::sanitize::sanitize_inline(message));
        }
    }

    /// Log entries landed from the provider.
    pub fn history_loaded(&mut self, entries: Vec<rootle_provider::LogEntry>, truncated: bool) {
        if let Some(h) = &mut self.history {
            h.loaded(entries, truncated);
        }
    }

    /// Optional file constraint for diagnostics; requests carry the full identity.
    pub fn history_path(&self) -> Option<&str> {
        self.history_request()
            .and_then(|request| request.scope.path())
    }

    pub fn history_active(&self) -> bool {
        self.history.is_some()
    }

    /// History lens `/` session active (keys route to the filter).
    pub fn history_filtering(&self) -> bool {
        self.history
            .as_ref()
            .is_some_and(|history| history.filter.active())
    }

    /// Begin the `/` session (remembers the committed filter so the
    /// session's Esc restores it).
    pub fn history_begin_filter(&mut self) {
        if let Some(h) = &mut self.history {
            h.filter.begin();
        }
    }

    /// A key for the filter session; commits/cancels end it. The list
    /// narrows as you type (pane-style live filter, not commit-only).
    pub fn history_filter_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        if let Some(history) = &mut self.history
            && history.filter.handle_key(key) != FilterOutcome::Unchanged
        {
            history.selection.reset();
            history.viewport.reset();
        }
    }

    /// Esc in the lens: a committed filter clears first (the wizard
    /// ladder), the second Esc closes. Returns true when it closed.
    pub fn history_esc(&mut self) -> bool {
        if let Some(h) = &mut self.history
            && h.filter.clear()
        {
            h.selection.reset();
            h.viewport.reset();
            return false;
        }
        self.close_history();
        true
    }

    pub fn history_move(&mut self, delta: i64) {
        if let Some(h) = &mut self.history {
            let movement = if delta < 0 {
                ListMovement::Previous
            } else {
                ListMovement::Next
            };
            h.selection
                .advance(movement, h.visible().len(), Boundary::Wrap);
        }
    }

    pub fn at_commit_view(&self) -> bool {
        self.at_commit.is_some()
    }

    /// The selected commit's typed identity and sanitized display metadata.
    pub fn history_pick_entry(&self) -> Option<HistoryEntry> {
        self.history.as_ref()?.selected().cloned()
    }

    pub fn history_pick(&self) -> Option<(HistoryRequest, Sha)> {
        let history = self.history.as_ref()?;
        Some((
            history.request.clone(),
            history.selected()?.revision.clone(),
        ))
    }

    /// The repository/file log, rendered with shared filter and viewport mechanics.
    pub(crate) fn render_history(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let Some(h) = &mut self.history else {
            return;
        };
        let sem = &theme.semantic;
        let label = match &h.request.scope {
            HistoryScope::Repository => format!("repo history — {}", h.request.repository),
            HistoryScope::File(path) => format!("history — {path}"),
        };
        let mut title = format!(" {} ", crate::sanitize::sanitize_inline(&label));
        if h.filter.active() || !h.filter.is_empty() {
            title = format!("{title}/{} ", h.filter.text());
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(title, Style::default().fg(sem.subtext0)));
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
        if let Some(message) = &h.failure {
            frame.render_widget(
                Paragraph::new(message.as_str()).style(Style::default().fg(sem.error)),
                inner,
            );
            return;
        }
        let vis = h.visible();
        let mut lines: Vec<Line> = Vec::new();
        for (row, &i) in vis.iter().enumerate() {
            let c = &h.entries[i];
            let selected = row == h.selection.selected().get();
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
                    format!("{} ", crate::sanitize::sanitize_inline(&c.revision.short())),
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
        let selected = (!vis.is_empty()).then(|| {
            let row = h.selection.selected().get() * 2;
            RowSpan::new(row, row + 2)
        });
        h.viewport
            .render(frame, area, inner, lines, selected, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_history_identity_does_not_depend_on_pane_captions() {
        let mut browser = Browser::new(&["owner".into()]);
        browser.org_repos_loaded("owner", vec![rootle_provider::RepoInfo::bare("project")]);
        browser.focus = 1;
        browser.levels[1].title = "display caption, not a repository owner".into();
        assert!(browser.open_repository_history());
        assert_eq!(
            browser.history_request().unwrap().repository,
            RepoId::from("owner/project")
        );
    }
}
