//! Global search view (plans/0002-v0.2.md §1, §5): full-screen Zed-style
//! search that replaces the browser while open. Fields row on top
//! (query · scope · extension), results below with one block per hit —
//! full path, then preview lines under it. `␣ f` = file find,
//! `␣ g` = grep. Stage 1 runs on mock data; the backend swaps the
//! producer of `GlobalSearchResults`, not this component.

use super::pane::fit;
use super::vim_input::{Outcome, SubMode, VimInput};
use crate::action::Action;
use crate::mode::Mode;
use crate::provider::Provider;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    FileFind,
    Grep,
}

impl SearchKind {
    pub fn title(self) -> &'static str {
        match self {
            SearchKind::FileFind => " find file ",
            SearchKind::Grep => " grep ",
        }
    }

    /// Cache-edit slug for materialized mock files.
    pub fn slug(self) -> &'static str {
        match self {
            SearchKind::FileFind => "find",
            SearchKind::Grep => "grep",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The repo currently open in the browser.
    Repo,
    /// The org selected in the browser's top level.
    Org,
    /// All of GitHub.
    Global,
}

impl Scope {
    /// Persisted form in state.json.
    pub fn stored(self) -> &'static str {
        match self {
            Scope::Repo => "repo",
            Scope::Org => "org",
            Scope::Global => "global",
        }
    }

    pub fn from_stored(s: &str) -> Option<Scope> {
        match s {
            "repo" => Some(Scope::Repo),
            "org" => Some(Scope::Org),
            "global" => Some(Scope::Global),
            _ => None,
        }
    }
}

/// Raw backend result from a worker thread — converted to a
/// `SearchHit` on the UI thread (highlight boundary, like blobs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHit {
    pub repo: String,
    pub path: String,
    pub sha: String,
    pub branch: String,
    pub line: u32,
    pub preview: Vec<(u32, String)>,
    pub match_count: u32,
}

/// One result: full path + highlighted preview lines (`line_no`, `Line`).
/// Multiple matches in one file fold into a single block; `match_count`
/// carries the badge shown next to the path (0 = path match / unknown).
/// `body` is the materializable file content for the editor when no
/// blob sha is known (mock); real hits open via `sha` + `repo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Owning repo ("owner/name"); mock fills a stand-in, the stage-2
    /// backend fills the real one. Drives yank URLs and blob fetches.
    pub repo: String,
    pub path: String,
    /// Blob sha (empty for mock hits) + the repo's default branch.
    pub sha: String,
    pub branch: String,
    pub line: u32,
    pub preview: Vec<(u32, Line<'static>)>,
    pub match_count: u32,
    pub body: String,
}

impl SearchHit {
    /// Unhighlighted hit (mock producer, tests): plain-text Lines.
    pub fn plain(
        repo: &str,
        path: &str,
        line: u32,
        preview: Vec<(u32, String)>,
        match_count: u32,
        body: String,
    ) -> Self {
        SearchHit {
            repo: repo.into(),
            path: path.into(),
            sha: String::new(),
            branch: "main".into(),
            line,
            preview: preview
                .into_iter()
                .map(|(no, text)| (no, Line::from(Span::raw(text))))
                .collect(),
            match_count,
            body,
        }
    }

    /// A raw backend hit, still unhighlighted (UI thread styles it).
    pub fn from_raw(raw: RawHit) -> Self {
        let mut hit = SearchHit::plain(
            &raw.repo,
            &raw.path,
            raw.line,
            raw.preview,
            raw.match_count,
            String::new(),
        );
        hit.sha = raw.sha;
        hit.branch = raw.branch;
        hit
    }

    /// Preview text, one line per entry — the highlighter's input.
    pub fn preview_text(&self) -> String {
        self.preview
            .iter()
            .map(|(_, line)| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Replace the preview styling while keeping line numbers.
    pub fn with_highlighted(self, lines: Vec<Line<'static>>) -> Self {
        let preview = self
            .preview
            .into_iter()
            .zip(lines)
            .map(|((no, _), line)| (no, line))
            .collect();
        SearchHit { preview, ..self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Query,
    Scope,
    Extension,
    Results,
}

const FOCUS_ORDER: [Focus; 4] = [Focus::Query, Focus::Scope, Focus::Extension, Focus::Results];

pub struct GlobalSearch {
    kind: SearchKind,
    /// Browser's open repo ("owner/name"); gates the Repo scope.
    repo: Option<String>,
    /// Browser's selected org; gates the Org scope.
    org: Option<String>,
    pub query: VimInput,
    extension: VimInput,
    scope: Scope,
    focus: Focus,
    /// Scope radio popup open.
    scope_popup: bool,
    /// Cursor inside the scope popup (index into `scope_items`).
    scope_cursor: usize,
    /// Scope when the popup opened; Esc reverts to it (the radio
    /// follows the cursor live, so cancel needs the original).
    scope_pre_popup: Scope,
    hits: Vec<SearchHit>,
    /// `/` transient filter over the results (path + preview text).
    filter: VimInput,
    filtering: bool,
    pre_filter: String,
    filter_value: String,
    /// Selected hit within the visible set.
    selected: usize,
    /// Line scroll offset of the results area (J/K free scroll).
    scroll: u16,
    pending: bool,
    error: Option<String>,
    submitted_once: bool,
}

impl GlobalSearch {
    /// The scope waterfalls from the current browser context: an open
    /// repo defaults to Repo, otherwise a selected org to Org,
    /// otherwise Global. A persisted scope (state.json) wins when its
    /// context is still available; same for the extension field.
    pub fn new(
        kind: SearchKind,
        repo: Option<String>,
        org: Option<String>,
        persisted_scope: Option<Scope>,
        persisted_extension: Option<String>,
    ) -> Self {
        let waterfall = if repo.is_some() {
            Scope::Repo
        } else if org.is_some() {
            Scope::Org
        } else {
            Scope::Global
        };
        let enabled = |s: Scope| match s {
            Scope::Repo => repo.is_some(),
            Scope::Org => org.is_some(),
            Scope::Global => true,
        };
        let scope = persisted_scope.filter(|s| enabled(*s)).unwrap_or(waterfall);
        let mut extension = VimInput::new();
        if let Some(ext) = persisted_extension.filter(|e| !e.is_empty()) {
            extension.prefill(&ext); // replaceable: typing starts fresh
        }
        GlobalSearch {
            kind,
            scope,
            repo,
            org,
            query: VimInput::new(),
            extension,
            focus: Focus::Query,
            scope_popup: false,
            scope_cursor: 0,
            scope_pre_popup: Scope::Global,
            hits: vec![],
            filter: VimInput::transient(),
            filtering: false,
            pre_filter: String::new(),
            filter_value: String::new(),
            selected: 0,
            scroll: 0,
            pending: false,
            error: None,
            submitted_once: false,
        }
    }

    pub fn kind(&self) -> SearchKind {
        self.kind
    }

    /// Current scope (for persistence on submit).
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// Current extension field value (for persistence on submit).
    pub fn extension_value(&self) -> String {
        self.extension.value()
    }

    /// (scope, enabled) radio rows for the scope popup.
    fn scope_items(&self) -> [(Scope, bool); 3] {
        [
            (Scope::Repo, self.repo.is_some()),
            (Scope::Org, self.org.is_some()),
            (Scope::Global, true),
        ]
    }

    fn scope_label(&self) -> String {
        match self.scope {
            Scope::Repo => match &self.repo {
                Some(repo) => format!("repo:{repo}"),
                None => "repo: —".into(),
            },
            Scope::Org => match &self.org {
                Some(org) => format!("org:{org}"),
                None => "org: —".into(),
            },
            Scope::Global => "global".into(),
        }
    }

    /// Modeline context: effective query summary (plans/0002 §2).
    pub fn context(&self) -> String {
        let what = match self.kind {
            SearchKind::FileFind => "find",
            SearchKind::Grep => "grep",
        };
        let mut ctx = format!("{what} · {}", self.scope_label());
        if !self.extension.value().is_empty() {
            ctx.push_str(&format!(" · ext:{}", self.extension.value()));
        }
        ctx
    }

    /// Hits surviving the committed `/` filter (path or preview text,
    /// case-insensitive substring — same rule as Pane::visible).
    fn visible(&self) -> Vec<&SearchHit> {
        let needle = self.filter_value.to_lowercase();
        self.hits
            .iter()
            .filter(|h| {
                needle.is_empty()
                    || h.path.to_lowercase().contains(&needle)
                    || h.preview
                        .iter()
                        .any(|(_, line)| line_text(line).to_lowercase().contains(&needle))
            })
            .collect()
    }

    pub fn selected_hit(&self) -> Option<&SearchHit> {
        self.visible().get(self.selected).copied()
    }

    fn set_filter(&mut self, value: String) {
        self.filter_value = value;
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.visible().len() as i32;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).clamp(0, len - 1) as usize;
    }

    fn cycle_focus(&mut self, reverse: bool) {
        let idx = FOCUS_ORDER
            .iter()
            .position(|f| *f == self.focus)
            .unwrap_or(0);
        let len = FOCUS_ORDER.len();
        let next = if reverse {
            (idx + len - 1) % len
        } else {
            (idx + 1) % len
        };
        self.focus = FOCUS_ORDER[next];
        // Focusing a text field always lands in INSERT (plans/0001 §1).
        match self.focus {
            Focus::Query => self.query.submode = SubMode::Insert,
            Focus::Extension => self.extension.submode = SubMode::Insert,
            _ => {}
        }
    }

    /// Move to the next/previous enabled scope without the popup.
    fn cycle_scope(&mut self, delta: i32) {
        let items = self.scope_items();
        let from = items
            .iter()
            .position(|(s, _)| *s == self.scope)
            .unwrap_or(0);
        let idx = next_enabled(&items, from, delta);
        if let Some((scope, true)) = items.get(idx).copied() {
            self.scope = scope;
        }
    }

    fn submit(&self) -> Action {
        Action::GlobalSearchSubmitted {
            kind: self.kind,
            query: self.query.value(),
            scope: self.scope_label(),
            extension: self.extension.value(),
        }
    }

    /// Modeline chip while the view is open (plans/0002 §2).
    pub fn effective_mode(&self) -> Mode {
        if self.filtering {
            return Mode::Search;
        }
        match self.focus {
            Focus::Query => match self.query.submode {
                SubMode::Insert => Mode::Insert,
                SubMode::Normal => Mode::Normal,
            },
            Focus::Extension => match self.extension.submode {
                SubMode::Insert => Mode::Insert,
                SubMode::Normal => Mode::Normal,
            },
            Focus::Scope | Focus::Results => Mode::Browse,
        }
    }

    /// Cursor shape for the focused text input (PLAN.md §5); hidden
    /// for the scope field and results.
    pub fn cursor_style(&self) -> Option<SetCursorStyle> {
        let input = match self.focus {
            Focus::Query => &self.query,
            Focus::Extension => &self.extension,
            _ => return None,
        };
        Some(match input.submode {
            SubMode::Insert => SetCursorStyle::SteadyBar,
            SubMode::Normal => SetCursorStyle::SteadyBlock,
        })
    }

    pub fn update(&mut self, action: &Action) {
        match action {
            Action::GlobalSearchSubmitted { .. } => {
                self.submitted_once = true;
                self.pending = true;
                self.error = None;
                self.focus = Focus::Results;
                self.selected = 0;
                self.scroll = 0;
            }
            Action::GlobalSearchResults { hits } => {
                self.pending = false;
                self.hits = hits.clone();
                self.selected = 0;
                self.scroll = 0;
                self.clamp_selection();
            }
            Action::GlobalSearchFailed { message } => {
                self.pending = false;
                self.error = Some(message.clone());
                self.hits = vec![];
            }
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Active `/` filter session captures everything until commit/cancel.
        if self.filtering {
            return match self.filter.handle_key(key) {
                Outcome::Changed => {
                    self.set_filter(self.filter.value());
                    Action::Noop
                }
                Outcome::Submitted => {
                    self.filtering = false; // commit: filter stays applied
                    Action::Noop
                }
                Outcome::Cancelled => {
                    self.set_filter(self.pre_filter.clone());
                    self.filtering = false;
                    Action::Noop
                }
                Outcome::Noop => Action::Noop,
            };
        }

        // Scope radio popup captures keys while open.
        if self.scope_popup {
            return self.scope_popup_key(key);
        }

        if key.code == KeyCode::Tab {
            self.cycle_focus(false);
            return Action::Noop;
        }
        if key.code == KeyCode::BackTab {
            self.cycle_focus(true);
            return Action::Noop;
        }

        match self.focus {
            Focus::Query => match self.query.handle_key(key) {
                Outcome::Submitted => {
                    self.filter.clear();
                    self.filter_value.clear();
                    self.submit()
                }
                Outcome::Cancelled => Action::CloseSearchView,
                _ => Action::Noop,
            },
            Focus::Extension => match self.extension.handle_key(key) {
                Outcome::Submitted => {
                    self.filter.clear();
                    self.filter_value.clear();
                    self.submit()
                }
                Outcome::Cancelled => Action::CloseSearchView,
                _ => Action::Noop,
            },
            Focus::Scope => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => {
                    // Open the popup with the cursor on the active scope.
                    let items = self.scope_items();
                    self.scope_cursor = items
                        .iter()
                        .position(|(s, _)| *s == self.scope)
                        .unwrap_or(0);
                    self.scope_pre_popup = self.scope;
                    self.scope_popup = true;
                    Action::Noop
                }
                // Cycle scopes right on the field (popup not required).
                KeyCode::Char('j') | KeyCode::Down => {
                    self.cycle_scope(1);
                    Action::Noop
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.cycle_scope(-1);
                    Action::Noop
                }
                KeyCode::Esc => Action::CloseSearchView,
                _ => Action::Noop,
            },
            Focus::Results => match key.code {
                // Leader layer works over the search view too (yank,
                // re-search); App routes leader keys while it's up.
                KeyCode::Char(' ') => Action::Leader,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.move_selection(1);
                    Action::Noop
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.move_selection(-1);
                    Action::Noop
                }
                KeyCode::Char('J') => {
                    self.scroll = self.scroll.saturating_add(1);
                    Action::Noop
                }
                KeyCode::Char('K') => {
                    self.scroll = self.scroll.saturating_sub(1);
                    Action::Noop
                }
                KeyCode::Char('/') => {
                    self.pre_filter = self.filter_value.clone();
                    self.filter.set(&self.pre_filter);
                    self.filtering = true;
                    Action::Noop
                }
                KeyCode::Enter => match self.selected_hit() {
                    Some(hit) => Action::OpenSearchHit(hit.clone()),
                    None => Action::Noop,
                },
                // Committed filter? First Esc clears it, second closes.
                KeyCode::Esc if !self.filter_value.is_empty() => {
                    self.set_filter(String::new());
                    Action::Noop
                }
                KeyCode::Esc => Action::CloseSearchView,
                _ => Action::Noop,
            },
        }
    }

    /// Scope popup keys: the radio follows the cursor live (j/k, g/G),
    /// Enter commits by closing, Esc reverts to the pre-popup scope.
    fn scope_popup_key(&mut self, key: KeyEvent) -> Action {
        let items = self.scope_items();
        match key.code {
            KeyCode::Esc => {
                self.scope = self.scope_pre_popup;
                self.scope_popup = false;
                Action::Noop
            }
            KeyCode::Enter => {
                self.scope_popup = false;
                Action::Noop
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_scope_cursor(next_enabled(&items, self.scope_cursor, 1));
                Action::Noop
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_scope_cursor(next_enabled(&items, self.scope_cursor, -1));
                Action::Noop
            }
            KeyCode::Char('g') => {
                self.move_scope_cursor(next_enabled(&items, items.len() - 1, 1));
                Action::Noop
            }
            KeyCode::Char('G') => {
                self.move_scope_cursor(next_enabled(&items, 0, -1));
                Action::Noop
            }
            _ => Action::Noop,
        }
    }

    /// Move the popup cursor and apply the scope it lands on.
    fn move_scope_cursor(&mut self, idx: usize) {
        self.scope_cursor = idx;
        if let Some((scope, true)) = self.scope_items().get(idx).copied() {
            self.scope = scope;
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;

        let hint = if self.filtering {
            " type to filter · enter commit · esc cancel "
        } else if self.scope_popup {
            " j/k move · enter done · esc revert "
        } else {
            " tab fields · enter search/open · / filter · esc close "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                self.kind.title(),
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(hint, Style::default().fg(sem.hint)));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(inner);

        let fields = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[0]);

        self.render_field(
            frame,
            fields[0],
            theme,
            " query ",
            &self.query.value(),
            self.focus == Focus::Query,
            Some(self.query.cursor()),
        );
        self.render_field(
            frame,
            fields[1],
            theme,
            " scope ",
            &format!("{} ▾", self.scope_label()),
            self.focus == Focus::Scope,
            None,
        );
        self.render_field(
            frame,
            fields[2],
            theme,
            " extension ",
            &self.extension.value(),
            self.focus == Focus::Extension,
            Some(self.extension.cursor()),
        );

        self.render_results(frame, rows[1], theme);

        if self.scope_popup {
            self.render_scope_popup(frame, area, theme);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        title: &str,
        value: &str,
        focused: bool,
        cursor: Option<usize>,
    ) {
        let sem = &theme.semantic;
        let border = if focused {
            sem.border_focused
        } else {
            sem.border_unfocused
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(title, Style::default().fg(sem.subtext0)));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let width = inner.width as usize;
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                fit(value, width),
                Style::default().fg(sem.text),
            ))),
            inner,
        );
        if focused {
            if let Some(cursor) = cursor {
                let x = inner.x + cursor as u16;
                if x < inner.x + inner.width {
                    frame.set_cursor_position((x, inner.y));
                }
            }
        }
    }

    fn render_results(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let focused = self.focus == Focus::Results;
        let border = if focused {
            sem.border_focused
        } else {
            sem.border_unfocused
        };

        let mut title = if self.pending {
            " results — searching… ".to_string()
        } else if let Some(error) = &self.error {
            format!(" results — error: {error} ")
        } else if self.hits.is_empty() && self.submitted_once {
            " results — no matches ".into()
        } else if self.submitted_once {
            format!(" results — {} ", self.visible().len())
        } else {
            " results ".into()
        };
        if !self.filter_value.is_empty() {
            title = format!("{} /{}", title.trim_end(), self.filter_value);
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(title, Style::default().fg(sem.subtext0)));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let width = inner.width as usize;
        let height = inner.height as usize;
        let visible = self.visible();

        // Build one block of lines per hit; remember each hit's line
        // range so the selection can be kept in view.
        let mut lines: Vec<Line> = Vec::new();
        let mut ranges: Vec<(usize, usize)> = Vec::new(); // [start, end)
        for (idx, hit) in visible.iter().enumerate() {
            let start = lines.len();
            let selected = idx == self.selected && focused;
            lines.push(self.path_line(hit, width, selected, theme));
            // Disjoint match regions get a dim ellipsis separator.
            let mut prev_no: Option<u32> = None;
            for (no, line) in &hit.preview {
                if let Some(prev) = prev_no {
                    if *no > prev + 1 {
                        lines.push(Line::from(Span::styled(
                            "       ⋮",
                            Style::default().fg(sem.subtext0),
                        )));
                    }
                }
                prev_no = Some(*no);
                lines.push(preview_line(*no, line, theme));
            }
            lines.push(Line::raw(""));
            ranges.push((start, lines.len() - 1));
        }
        let total = lines.len();

        // Keep the selected hit visible; J/K free scroll otherwise.
        if focused {
            if let Some((start, end)) = ranges.get(self.selected).copied() {
                if start < self.scroll as usize {
                    self.scroll = start as u16;
                } else if end >= self.scroll as usize + height {
                    self.scroll = (end + 1).saturating_sub(height) as u16;
                }
            }
        }
        let max_scroll = total.saturating_sub(height) as u16;
        self.scroll = self.scroll.min(max_scroll);

        frame.render_widget(Paragraph::new(lines).scroll((self.scroll, 0)), inner);
    }

    fn path_line(
        &self,
        hit: &SearchHit,
        width: usize,
        selected: bool,
        theme: &Theme,
    ) -> Line<'static> {
        let sem = &theme.semantic;
        let gutter = if selected { "▌ " } else { "  " };
        // Grep hits carry a match-count badge (folded multi-matches);
        // file-find hits show the first line number instead.
        let meta = if hit.match_count > 0 {
            format!(
                "{} match{}",
                hit.match_count,
                if hit.match_count == 1 { "" } else { "es" }
            )
        } else {
            format!(":{}", hit.line)
        };
        // Cross-repo results need the repo in the row; repo-scope
        // results keep it too — unambiguous everywhere.
        let full = format!("{}/{}", hit.repo, hit.path);
        let path_width = width.saturating_sub(2 + meta.width());
        let path = fit(&full, path_width);
        let pad = width.saturating_sub(2 + path.width() + meta.width());
        let (fg, bg) = if selected {
            (sem.selection_fg, Some(sem.selection_bg))
        } else {
            (sem.text, None)
        };
        let style = {
            let mut s = Style::default().fg(fg).add_modifier(Modifier::BOLD);
            if let Some(bg) = bg {
                s = s.bg(bg);
            }
            s
        };
        let meta_style = {
            let mut s = Style::default().fg(sem.subtext0);
            if let Some(bg) = bg {
                s = s.bg(bg);
            }
            s
        };
        Line::from(vec![
            Span::styled(gutter, Style::default().fg(sem.border_focused)),
            Span::styled(path, style),
            Span::styled(" ".repeat(pad), meta_style),
            Span::styled(meta, meta_style),
        ])
    }

    fn render_scope_popup(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let items = self.scope_items();
        let height = items.len() as u16 + 2; // rows + border
        let popup_area = super::centered(area, 40, 30);
        let popup = Rect {
            height: height.min(popup_area.height),
            ..popup_area
        };

        frame.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                " scope ",
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                " j/k move · enter done · esc revert ",
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let mut lines = Vec::new();
        for (idx, (scope, enabled)) in items.iter().enumerate() {
            let radio = if *scope == self.scope { "(•)" } else { "( )" };
            let label = match scope {
                Scope::Repo => match &self.repo {
                    Some(repo) => format!("current repo  repo:{repo}"),
                    None => "current repo  (no repo open)".to_string(),
                },
                Scope::Org => match &self.org {
                    Some(org) => format!("current org  org:{org}"),
                    None => "current org  (no org selected)".to_string(),
                },
                Scope::Global => "all of github".to_string(),
            };
            let cursor = idx == self.scope_cursor;
            let fg = if !enabled {
                sem.subtext0
            } else if cursor {
                sem.selection_fg
            } else {
                sem.text
            };
            let mut style = Style::default().fg(fg);
            if cursor {
                style = style.bg(sem.selection_bg);
            }
            lines.push(Line::from(Span::styled(
                format!("{} {}", radio, label),
                style,
            )));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Next enabled radio index, wrapping; skips disabled items.
fn next_enabled(items: &[(Scope, bool)], from: usize, delta: i32) -> usize {
    let len = items.len() as i32;
    let mut idx = from as i32;
    for _ in 0..len {
        idx = (idx + delta).rem_euclid(len);
        if items[idx as usize].1 {
            return idx as usize;
        }
    }
    from
}

/// Plain-text content of a rendered line (for `/` filtering).
fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Grep: restyle occurrences of `query` inside preview lines with the
/// theme's match chip (search_match bg, crust fg). Stage 2 will use the
/// API's text-match ranges instead of re-finding the substring.
/// Byte offsets come from the lowercased text — exact for ASCII,
/// cosmetic-only drift on exotic unicode case folds.
pub fn highlight_matches(
    hits: &mut [SearchHit],
    query: &str,
    match_bg: ratatui::style::Color,
    match_fg: ratatui::style::Color,
) {
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return;
    }
    let chip = Style::default()
        .fg(match_fg)
        .bg(match_bg)
        .add_modifier(Modifier::BOLD);
    for hit in hits {
        for (_, line) in &mut hit.preview {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for span in &line.spans {
                let text = span.content.to_string();
                let lower = text.to_lowercase();
                let mut at = 0; // byte offset into `text`
                let mut rest = lower.as_str();
                while let Some(pos) = rest.find(&needle) {
                    let (start, end) = (at + pos, at + pos + needle.len());
                    if start > at {
                        spans.push(Span::styled(text[at..start].to_string(), span.style));
                    }
                    spans.push(Span::styled(text[start..end].to_string(), chip));
                    at = end;
                    rest = &lower[end..];
                }
                if at < text.len() {
                    spans.push(Span::styled(text[at..].to_string(), span.style));
                }
            }
            *line = Line::from(spans);
        }
    }
}

/// Preview line: right-aligned line-number gutter + the highlighted
/// code spans (clipped at the area width, like the browser preview).
fn preview_line(no: u32, line: &Line<'static>, theme: &Theme) -> Line<'static> {
    let sem = &theme.semantic;
    let gutter = format!("  {:>4}  ", no);
    let mut spans = vec![Span::styled(gutter, Style::default().fg(sem.subtext0))];
    spans.extend(line.spans.iter().cloned());
    Line::from(spans)
}

/// Mock producer (plans/0002 §5): tests and the offline app inject
/// these; the real backend below feeds the same `SearchHit` shape.
pub mod mock {
    use super::{SearchHit, SearchKind};

    /// (path, first-match line, preview lines as (line_no, text)).
    type MockFile = (&'static str, u32, &'static [(u32, &'static str)]);

    /// Mock hits honoring the UI inputs the way the stage-2 backend
    /// will: file find matches the query against paths, grep against
    /// paths + matched lines, and `extension` filters by suffix.
    pub fn hits(kind: SearchKind, query: &str, extension: &str) -> Vec<SearchHit> {
        let bodies: &[MockFile] = match kind {
            SearchKind::Grep => &[
                (
                    "src/widgets/list.rs",
                    42,
                    &[
                        (
                            40,
                            "pub fn render(mut self, area: Rect, buf: &mut Buffer) {",
                        ),
                        (41, "    let items = self.items.into_iter()"),
                        (42, "        .filter(|item| item.matches(query))"),
                        (43, "        .collect::<Vec<_>>();"),
                        (44, "    self.render_items(items, area, buf);"),
                        // Second region in the same file — folds into
                        // this block behind an ellipsis separator.
                        (88, "fn rerank(hits: &mut [Hit], query: &str) {"),
                        (89, "    hits.sort_by_key(|hit| hit.score(query));"),
                    ],
                ),
                (
                    "src/terminal.rs",
                    137,
                    &[
                        (135, "    /// Flush the diff to the terminal."),
                        (136, "    pub fn flush(&mut self) -> io::Result<()> {"),
                        (137, "        let query = self.frame_query.take();"),
                        (138, "        self.backend.draw(query.iter())?;"),
                        (139, "        Ok(())"),
                    ],
                ),
                (
                    "src/components/global_search.rs",
                    12,
                    &[
                        (10, "//! Global search view: fields on top,"),
                        (11, "//! Zed-style result blocks below."),
                        (12, "pub fn query(&self) -> &str {"),
                        (13, "    self.query.value()"),
                        (14, "}"),
                    ],
                ),
                (
                    "docs/keymap.md",
                    3,
                    &[
                        (1, "# Keymap"),
                        (2, ""),
                        (3, "Every query starts from the leader key."),
                        (4, "Tab cycles the field row."),
                    ],
                ),
            ],
            SearchKind::FileFind => &[
                (
                    "src/query/parser.rs",
                    1,
                    &[
                        (1, "use crate::query::ast::Expr;"),
                        (2, ""),
                        (3, "pub fn parse(input: &str) -> Result<Expr, Error> {"),
                        (4, "    Parser::new(input).expr()"),
                    ],
                ),
                (
                    "src/query/ast.rs",
                    1,
                    &[
                        (1, "pub enum Expr {"),
                        (2, "    Term(String),"),
                        (3, "    And(Box<Expr>, Box<Expr>),"),
                        (4, "    Or(Box<Expr>, Box<Expr>),"),
                    ],
                ),
                (
                    "tests/query_roundtrip.rs",
                    1,
                    &[
                        (1, "#[test]"),
                        (2, "fn query_roundtrip() {"),
                        (3, "    let q = \"repo:ratatui ext:rs\";"),
                        (4, "    assert_eq!(parse(q).to_string(), q);"),
                    ],
                ),
            ],
        };
        let needle = query.to_lowercase();
        let ext = extension.trim_start_matches('.').to_lowercase();
        bodies
            .iter()
            .filter(|(path, _, preview)| {
                let path = path.to_lowercase();
                let matches_query = needle.is_empty()
                    || match kind {
                        SearchKind::FileFind => path.contains(&needle),
                        SearchKind::Grep => {
                            path.contains(&needle)
                                || preview
                                    .iter()
                                    .any(|(_, text)| text.to_lowercase().contains(&needle))
                        }
                    };
                let matches_ext = ext.is_empty() || path.ends_with(&format!(".{ext}"));
                matches_query && matches_ext
            })
            .map(|(path, line, preview)| {
                let match_count = match kind {
                    // Occurrences of the query across the preview lines.
                    SearchKind::Grep if !needle.is_empty() => preview
                        .iter()
                        .map(|(_, text)| text.to_lowercase().matches(&needle).count() as u32)
                        .sum(),
                    _ => 0, // file find: path match, no content badge
                };
                SearchHit::plain(
                    "ratatui/ratatui", // mock repo; stage 2 fills the real one
                    path,
                    *line,
                    preview
                        .iter()
                        .map(|(no, text)| (*no, text.to_string()))
                        .collect(),
                    match_count,
                    mock_body(path, preview, query),
                )
            })
            .collect()
    }

    /// Full-file stand-in: the preview lines plus surrounding filler so
    /// the editor opens on something that looks real.
    fn mock_body(path: &str, preview: &[(u32, &str)], query: &str) -> String {
        let mut body = format!("// mock content for {path} (stage 1, query: {query:?})\n");
        for (_, text) in preview {
            body.push_str(text);
            body.push('\n');
        }
        body.push_str("// …stage 2 replaces this with the fetched blob.\n");
        body
    }
}

// ---------------------------------------------------------------------------
// Real backend (plans/0002 §4, milestones 2–3). Runs on a worker thread;
// everything here is pure I/O → RawHit, styling happens on the UI thread.
// ---------------------------------------------------------------------------

/// How many hits get a blob-located preview (fetch cost; rest render
/// as bare paths). Cache-first, so repeat searches are free.
const PREVIEW_CAP: usize = 8;
/// Max results kept from a code-search page.
const HIT_CAP: usize = 25;

/// Build the `/search/code` query: file find matches paths, grep
/// matches content; scope/ext map to GitHub qualifiers.
fn code_query(kind: SearchKind, query: &str, scope_label: &str, extension: &str) -> String {
    let mut q = match kind {
        SearchKind::Grep => query.to_string(),
        SearchKind::FileFind => format!("path:{query}"),
    };
    if scope_label != "global" {
        q.push(' ');
        q.push_str(scope_label); // "repo:o/r" / "org:x" — valid qualifiers
    }
    let ext = extension.trim_start_matches('.');
    if !ext.is_empty() {
        q.push_str(&format!(" extension:{ext}"));
    }
    q
}

/// Entry point for the view's worker (plans/0002 §4): repo-scoped file
/// find runs over the cached tree (no search-API spend); everything
/// else goes through /search/code.
pub fn run_view_search(
    provider: &dyn Provider,
    kind: SearchKind,
    query: &str,
    scope_label: &str,
    extension: &str,
) -> Result<Vec<RawHit>, String> {
    if kind == SearchKind::FileFind && scope_label.starts_with("repo:") {
        return tree_file_find(provider, query, &scope_label["repo:".len()..], extension);
    }
    code_search(provider, kind, query, scope_label, extension)
}

/// File find over the repo's cached recursive tree — substring on
/// paths, blob heads as previews. Zero search-API calls.
fn tree_file_find(
    provider: &dyn Provider,
    query: &str,
    repo_full: &str,
    extension: &str,
) -> Result<Vec<RawHit>, String> {
    let tree = provider.fetch_tree(repo_full)?;
    let branch = tree.branch;
    let needle = query.to_lowercase();
    let ext = extension.trim_start_matches('.').to_lowercase();
    let mut hits = Vec::new();
    for entry in tree.entries {
        if entry.is_dir {
            continue;
        }
        let path = entry.path.to_lowercase();
        if !needle.is_empty() && !path.contains(&needle) {
            continue;
        }
        if !ext.is_empty() && !path.ends_with(&format!(".{ext}")) {
            continue;
        }
        hits.push(RawHit {
            repo: repo_full.to_string(),
            path: entry.path,
            sha: entry.sha,
            branch: branch.clone(),
            line: 1,
            preview: vec![],
            match_count: 0,
        });
        if hits.len() >= HIT_CAP {
            break;
        }
    }
    add_blob_heads(provider, &mut hits);
    Ok(hits)
}

/// /search/code for grep (content) and non-repo file find (path:).
fn code_search(
    provider: &dyn Provider,
    kind: SearchKind,
    query: &str,
    scope_label: &str,
    extension: &str,
) -> Result<Vec<RawHit>, String> {
    let q = code_query(kind, query, scope_label, extension);
    let items = provider.search_code(&q)?;
    let mut hits: Vec<RawHit> = Vec::new();
    for item in items.into_iter().take(HIT_CAP) {
        let needles = item.matches.clone();
        let mut hit = RawHit {
            repo: item.repo,
            path: item.path,
            sha: item.sha,
            branch: item.branch,
            line: 1,
            preview: vec![],
            match_count: needles.len() as u32,
        };
        // Grep: real line numbers come from locating the matched texts
        // in the blob (fragments carry no absolute numbers).
        if kind == SearchKind::Grep && hits.len() < PREVIEW_CAP {
            if let Some((line, preview, count)) =
                locate_matches(provider, &hit.repo, &hit.sha, &needles)
            {
                hit.line = line;
                hit.preview = preview;
                hit.match_count = count;
            }
        }
        hits.push(hit);
    }
    if kind == SearchKind::FileFind {
        add_blob_heads(provider, &mut hits);
    }
    Ok(hits)
}

/// (first match line, preview lines, matched-line count).
type LocatedPreview = (u32, Vec<(u32, String)>, u32);

/// Grep preview: fetch the blob (cache-first), find the lines matching
/// the query's needles, merge into ≤2 regions of ≤5 lines.
fn locate_matches(
    provider: &dyn Provider,
    repo: &str,
    sha: &str,
    needles: &[String],
) -> Option<LocatedPreview> {
    let bytes = provider.fetch_blob(repo, sha).ok()?;
    if crate::sanitize::is_binary(&bytes) {
        return None;
    }
    let text = crate::sanitize::sanitize(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let needles: Vec<String> = needles
        .iter()
        .map(|n| n.to_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    if needles.is_empty() {
        return None;
    }
    let matched: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let ll = l.to_lowercase();
            needles.iter().any(|n| ll.contains(n))
        })
        .map(|(i, _)| i)
        .collect();
    if matched.is_empty() {
        return None;
    }
    // Regions: matched lines with one context line each side; merge
    // when regions touch; cap 2 regions × 5 lines.
    let mut regions: Vec<(usize, usize)> = Vec::new();
    for &m in &matched {
        let (start, end) = (m.saturating_sub(1), (m + 2).min(lines.len()));
        match regions.last_mut() {
            Some((_, e)) if start <= *e => *e = end.max(*e),
            _ => regions.push((start, end)),
        }
    }
    let mut preview = Vec::new();
    for (start, end) in regions.into_iter().take(2) {
        let capped = end.min(start + 5);
        for (i, line) in lines.iter().enumerate().take(capped).skip(start) {
            preview.push(((i + 1) as u32, line.to_string()));
        }
    }
    Some(((matched[0] + 1) as u32, preview, matched.len() as u32))
}

/// File-find preview: the file's first lines from its blob.
fn add_blob_heads(provider: &dyn Provider, hits: &mut [RawHit]) {
    for hit in hits.iter_mut().take(PREVIEW_CAP) {
        let Ok(bytes) = provider.fetch_blob(&hit.repo, &hit.sha) else {
            continue;
        };
        if crate::sanitize::is_binary(&bytes) {
            continue;
        }
        let text = crate::sanitize::sanitize(&bytes);
        hit.preview = text
            .lines()
            .take(3)
            .enumerate()
            .map(|(i, l)| ((i + 1) as u32, l.to_string()))
            .collect();
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

    fn view() -> GlobalSearch {
        GlobalSearch::new(
            SearchKind::Grep,
            Some("ratatui/ratatui".into()),
            Some("ratatui".into()),
            None,
            None,
        )
    }

    fn submit(view: &mut GlobalSearch, query: &str) {
        for c in query.chars() {
            view.handle_key(key(KeyCode::Char(c)));
        }
        let action = view.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, Action::GlobalSearchSubmitted { .. }));
        view.update(&action);
        view.update(&Action::GlobalSearchResults {
            hits: mock::hits(SearchKind::Grep, query, ""),
        });
    }

    #[test]
    fn tab_cycles_all_four_focus_targets() {
        let mut v = view();
        assert_eq!(v.focus, Focus::Query);
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Scope);
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Extension);
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Results);
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, Focus::Query);
        v.handle_key(key(KeyCode::BackTab));
        assert_eq!(v.focus, Focus::Results);
    }

    #[test]
    fn enter_in_query_submits_and_focuses_results() {
        let mut v = view();
        submit(&mut v, "query");
        assert_eq!(v.focus, Focus::Results);
        assert_eq!(v.hits.len(), 4);
    }

    #[test]
    fn scope_popup_radio_follows_cursor_and_esc_reverts() {
        let mut v = view();
        v.handle_key(key(KeyCode::Tab)); // scope focused
        v.handle_key(key(KeyCode::Enter));
        assert!(v.scope_popup);
        // Radio follows the cursor down the waterfall: repo → org → global.
        v.handle_key(key(KeyCode::Char('j')));
        assert_eq!(v.scope, Scope::Org);
        v.handle_key(key(KeyCode::Char('j')));
        assert_eq!(v.scope, Scope::Global);
        v.handle_key(key(KeyCode::Esc)); // revert to the pre-popup scope
        assert!(!v.scope_popup);
        assert_eq!(v.scope, Scope::Repo);

        // Enter commits wherever the radio stands.
        v.handle_key(key(KeyCode::Enter));
        v.handle_key(key(KeyCode::Char('j')));
        v.handle_key(key(KeyCode::Char('j')));
        v.handle_key(key(KeyCode::Enter));
        assert!(!v.scope_popup);
        assert_eq!(v.scope, Scope::Global);
    }

    #[test]
    fn repo_and_org_scopes_disabled_without_context() {
        let mut v = GlobalSearch::new(SearchKind::FileFind, None, None, None, None);
        assert_eq!(v.scope, Scope::Global);
        v.handle_key(key(KeyCode::Tab));
        v.handle_key(key(KeyCode::Enter)); // open popup
        v.handle_key(key(KeyCode::Char('j'))); // wraps: repo + org skipped
        assert_eq!(v.scope_cursor, 2); // global stays the only target
        v.handle_key(key(KeyCode::Enter));
        assert_eq!(v.scope, Scope::Global);
    }

    #[test]
    fn scope_waterfalls_from_browser_context() {
        // Repo open → Repo; only org → Org; nothing → Global.
        let v = GlobalSearch::new(
            SearchKind::Grep,
            Some("ratatui/ratatui".into()),
            Some("ratatui".into()),
            None,
            None,
        );
        assert_eq!(v.scope, Scope::Repo);
        let v = GlobalSearch::new(SearchKind::Grep, None, Some("ratatui".into()), None, None);
        assert_eq!(v.scope, Scope::Org);
        assert_eq!(v.scope_label(), "org:ratatui");
        let v = GlobalSearch::new(SearchKind::Grep, None, None, None, None);
        assert_eq!(v.scope, Scope::Global);
    }

    #[test]
    fn slash_filter_narrows_and_esc_restores() {
        let mut v = view();
        submit(&mut v, "query");
        assert_eq!(v.focus, Focus::Results);
        v.handle_key(key(KeyCode::Char('/')));
        assert!(v.filtering);
        for c in "terminal".chars() {
            v.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(v.visible().len(), 1);
        assert_eq!(v.visible()[0].path, "src/terminal.rs");
        v.handle_key(key(KeyCode::Esc)); // cancel → pre-filter
        assert_eq!(v.visible().len(), 4);
    }

    #[test]
    fn effective_mode_follows_focus_and_submode() {
        let mut v = view();
        assert_eq!(v.effective_mode(), Mode::Insert);
        v.query.submode = SubMode::Normal;
        assert_eq!(v.effective_mode(), Mode::Normal);
        v.handle_key(key(KeyCode::Tab)); // scope
        assert_eq!(v.effective_mode(), Mode::Browse);
    }

    #[test]
    fn esc_closes_from_results() {
        let mut v = view();
        submit(&mut v, "query");
        let action = v.handle_key(key(KeyCode::Esc));
        assert_eq!(action, Action::CloseSearchView);
    }

    #[test]
    fn enter_on_hit_emits_open() {
        let mut v = view();
        submit(&mut v, "query");
        let action = v.handle_key(key(KeyCode::Enter));
        match action {
            Action::OpenSearchHit(hit) => assert_eq!(hit.path, "src/widgets/list.rs"),
            other => panic!("expected OpenSearchHit, got {other:?}"),
        }
    }

    #[test]
    fn scope_field_cycles_with_vim_motions() {
        let mut v = view();
        v.handle_key(key(KeyCode::Tab)); // scope focused
        assert_eq!(v.scope, Scope::Repo);
        v.handle_key(key(KeyCode::Char('j'))); // repo → org, no popup
        assert_eq!(v.scope, Scope::Org);
        v.handle_key(key(KeyCode::Char('j'))); // org → global
        assert_eq!(v.scope, Scope::Global);
        assert!(!v.scope_popup);
        v.handle_key(key(KeyCode::Char('k'))); // back to org
        assert_eq!(v.scope, Scope::Org);
        // Disabled scopes are skipped when no context is open.
        let mut v = GlobalSearch::new(SearchKind::Grep, None, None, None, None);
        v.handle_key(key(KeyCode::Tab));
        v.handle_key(key(KeyCode::Char('k')));
        assert_eq!(v.scope, Scope::Global);
    }

    #[test]
    fn multi_match_files_fold_with_count_badge() {
        let hits = mock::hits(SearchKind::Grep, "query", "");
        let list = hits
            .iter()
            .find(|h| h.path == "src/widgets/list.rs")
            .unwrap();
        assert_eq!(list.match_count, 3);
        // Both regions live in the one folded block.
        let nos: Vec<u32> = list.preview.iter().map(|(n, _)| *n).collect();
        assert!(nos.contains(&42));
        assert!(nos.contains(&88));
    }

    #[test]
    fn highlight_matches_chips_query_spans() {
        use ratatui::style::Color;
        let mut hits = mock::hits(SearchKind::Grep, "query", "");
        highlight_matches(&mut hits, "query", Color::Yellow, Color::Black);
        let hit = hits.iter().find(|h| h.path == "src/terminal.rs").unwrap();
        // "let query = self.frame_query.take();" — two occurrences.
        let line = &hit.preview[2].1;
        let chipped: Vec<&Span> = line
            .spans
            .iter()
            .filter(|s| s.style.bg == Some(Color::Yellow))
            .collect();
        assert_eq!(chipped.len(), 2);
        assert!(chipped.iter().all(|s| s.content.as_ref() == "query"));
    }

    #[test]
    fn mock_honors_query_and_extension() {
        // File find matches the query against paths.
        let hits = mock::hits(SearchKind::FileFind, "parser", "");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/query/parser.rs");
        // Empty query returns everything.
        assert_eq!(mock::hits(SearchKind::FileFind, "", "").len(), 3);
        // Grep matches paths and matched lines.
        assert_eq!(mock::hits(SearchKind::Grep, "query", "").len(), 4);
        assert_eq!(mock::hits(SearchKind::Grep, "flush", "").len(), 1);
        // Extension narrows by suffix (dot optional).
        let rs = mock::hits(SearchKind::Grep, "query", "rs");
        assert_eq!(rs.len(), 3);
        assert!(rs.iter().all(|h| h.path.ends_with(".rs")));
        assert_eq!(mock::hits(SearchKind::Grep, "query", ".md").len(), 1);
        // No matches is an honest empty state.
        assert!(mock::hits(SearchKind::Grep, "zzz", "").is_empty());
    }
}
