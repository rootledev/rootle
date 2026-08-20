//! One miller column: border + title + filterable entry list (PLAN.md §5).
//! Directories render blue+bold with trailing `/`, files in text color.

use crate::action::Action;
use crate::sanitize;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    /// Pseudo-entries: orgs, repos — rendered like dirs.
    Repo,
    Org,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
}

impl Entry {
    pub fn new(name: &str, kind: EntryKind) -> Self {
        Entry {
            name: sanitize::sanitize_inline(name),
            kind,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pane {
    pub title: String,
    pub entries: Vec<Entry>,
    pub filter: String,
    /// Render `[repo]`/`[org]` badges before entries (search results).
    pub show_badges: bool,
    /// VISUAL mode: entry names carrying a mark; `Some` enables mark
    /// rendering (plans/0004 §1).
    pub checkboxes: Option<std::collections::HashSet<String>>,
    /// With checkboxes set but visual OFF: show ● for marked entries
    /// only (marks persist after leaving visual; ○ would imply the
    /// rows are selectable right now).
    pub marks_only: bool,
    selected: usize,
    state: ListState,
    pub focused: bool,
}

impl Pane {
    pub fn new(title: impl Into<String>, entries: Vec<Entry>) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Pane {
            title: title.into(),
            entries,
            filter: String::new(),
            show_badges: false,
            checkboxes: None,
            marks_only: false,
            selected: 0,
            state,
            focused: false,
        }
    }

    /// Entries surviving the current filter (case-insensitive substring).
    pub fn visible(&self) -> Vec<&Entry> {
        let needle = self.filter.to_lowercase();
        self.entries
            .iter()
            .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle))
            .collect()
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.visible().get(self.selected).copied()
    }

    fn clamp(&mut self) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
        self.state.select(Some(self.selected));
    }

    pub fn move_by(&mut self, delta: i32) {
        let len = self.visible().len() as i32;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).clamp(0, len - 1) as usize;
        self.state.select(Some(self.selected));
    }

    pub fn select(&mut self, idx: usize) {
        self.selected = idx;
        self.clamp();
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.clamp();
    }

    pub fn update(&mut self, action: &Action) {
        match action {
            Action::MoveDown => self.move_by(1),
            Action::MoveUp => self.move_by(-1),
            _ => {}
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let border_style = if self.focused {
            Style::default().fg(sem.border_focused)
        } else {
            Style::default().fg(sem.border_unfocused)
        };

        let title = if self.filter.is_empty() {
            self.title.clone()
        } else {
            format!("{}  /{}", self.title, self.filter)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style)
            .style(Style::default().bg(sem.base))
            .title(Span::styled(
                format!(" {} ", title),
                Style::default().fg(sem.subtext0),
            ));

        let box_w = if self.checkboxes.is_some() { 4 } else { 0 };
        let width = area.width.saturating_sub(2 + box_w) as usize;
        let items: Vec<ListItem> = self
            .visible()
            .iter()
            .map(|e| {
                // VISUAL marker gutter, before any badge/name: a green
                // ● for marked entries, dim ○ otherwise.
                let checkbox = self.checkboxes.as_ref().map(|set| {
                    if set.contains(&e.name) {
                        Span::styled("●  ", Style::default().fg(sem.mode_browse))
                    } else if self.marks_only {
                        Span::raw("   ") // marked-only view: keep the gutter blank
                    } else {
                        Span::styled("○  ", Style::default().fg(sem.subtext0))
                    }
                });
                let line = match e.kind {
                    EntryKind::Repo | EntryKind::Org if self.show_badges => {
                        let (badge, color) = match e.kind {
                            EntryKind::Repo => ("[repo]", sem.badge_repo),
                            _ => ("[org]", sem.badge_org),
                        };
                        Line::from(vec![
                            Span::styled(
                                badge,
                                Style::default().fg(color).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!(" {}", fit(&e.name, width.saturating_sub(8))),
                                Style::default().fg(sem.text),
                            ),
                        ])
                    }
                    EntryKind::Dir | EntryKind::Repo | EntryKind::Org => Line::from(Span::styled(
                        fit(&format!("{}/", e.name), width),
                        Style::default()
                            .fg(sem.directory)
                            .add_modifier(Modifier::BOLD),
                    )),
                    EntryKind::File => Line::from(Span::styled(
                        fit(&e.name, width),
                        Style::default().fg(sem.file),
                    )),
                };
                let line = match checkbox {
                    Some(mark) => {
                        let mut spans = vec![mark];
                        spans.extend(line.spans);
                        Line::from(spans)
                    }
                    None => line,
                };
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(sem.selection_bg).fg(sem.selection_fg))
            .highlight_symbol("▌");
        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

/// Truncate to display width (CJK = 2 cells), never byte-count (PLAN.md §9).
pub(crate) fn fit(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        w += cw;
        out.push(c);
    }
    out.push('…');
    out
}
