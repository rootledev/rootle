//! Preview column: sanitized text for files, child listing for dirs.
//! Syntax highlighting lands in milestone 5; text is already sanitized.

use super::pane::{Entry, EntryKind};
use crate::sanitize;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

#[derive(Debug, Clone, Default)]
pub enum PreviewContent {
    #[default]
    Empty,
    Text(String),
    /// Syntax-highlighted lines (syntect → palette colors).
    Highlighted(Vec<Line<'static>>),
    DirSummary(Vec<Entry>),
    Binary {
        size: usize,
    },
}

/// One find occurrence: 0-based line + byte range in that line's
/// plain (tab-expanded) text.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FindMatch {
    line: u16,
    start: usize,
    end: usize,
}

/// Active find-in-file session (plans/0007 §3). Chips render from
/// `matches`; `n`/`N` move `current` and the line cursor follows.
struct FindState {
    query: String,
    matches: Vec<FindMatch>,
    /// Index into `matches` the cursor sits on.
    current: usize,
    /// Cursor line before the session — restored on cancel (vim `/`).
    saved_cursor: u16,
}

/// Case-insensitive substring matches across all lines, in occurrence
/// order. Byte offsets come from the lowercased text — exact for
/// ASCII, cosmetic-only drift on exotic unicode case folds (same
/// tradeoff as the grep view's chip pass).
fn compute_matches(lines: &[String], query: &str) -> Vec<FindMatch> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        let mut at = 0usize;
        while let Some(rest) = lower.get(at..) {
            let Some(pos) = rest.find(&q) else { break };
            let start = at + pos;
            out.push(FindMatch {
                line: i as u16,
                start,
                end: start + q.len(),
            });
            at = start + q.len();
        }
    }
    out
}

pub struct Preview {
    pub content: PreviewContent,
    pub title: String,
    /// Vertical scroll offset (lines), follows the line cursor.
    scroll: u16,
    /// Line cursor (0-based) — `J/K` walk it, `␣ y` anchors the yank
    /// URL to it (plans/0006 §5). Only text content is cursored.
    cursor: u16,
    /// Total logical lines of the current text content; 0 = cursorless.
    line_count: u16,
    /// Real file content gets the line-number gutter; meta placeholders
    /// ("loading…"), dirs and binaries don't (plans/0007 §4).
    numbered: bool,
    /// Language label for the footer (`rust · 41 lines`), when the
    /// highlighter resolved one.
    lang: Option<String>,
    /// Find-in-file session (`␣ /`); chips + `n`/`N` target.
    find: Option<FindState>,
}

impl Default for Preview {
    fn default() -> Self {
        Self::new()
    }
}

impl Preview {
    pub fn new() -> Self {
        Preview {
            content: PreviewContent::Empty,
            title: "preview".into(),
            scroll: 0,
            cursor: 0,
            line_count: 0,
            numbered: false,
            lang: None,
            find: None,
        }
    }

    /// Load raw bytes (as fetched from a blob); binary → placeholder.
    pub fn set_bytes(&mut self, name: &str, bytes: &[u8]) {
        self.title = sanitize::sanitize_inline(name);
        self.lang = None;
        if sanitize::is_binary(bytes) {
            self.content = PreviewContent::Binary { size: bytes.len() };
            self.line_count = 0;
            self.numbered = false;
        } else {
            let text = sanitize::sanitize(bytes);
            self.line_count = text.lines().count() as u16;
            self.content = PreviewContent::Text(text);
            self.numbered = true;
        }
        self.reset();
    }

    pub fn set_dir(&mut self, name: &str, children: Vec<Entry>) {
        self.title = format!("{}/", sanitize::sanitize_inline(name));
        self.content = PreviewContent::DirSummary(children);
        self.line_count = 0;
        self.numbered = false;
        self.lang = None;
        self.reset();
    }

    /// File meta until blob content lands (milestone 5): size + blob sha.
    pub fn set_file_meta(&mut self, name: &str, size: Option<u64>, sha: &str) {
        self.title = sanitize::sanitize_inline(name);
        let size = size.map(|s| s.to_string()).unwrap_or_else(|| "?".into());
        let short = &sha[..sha.len().min(7)];
        let text = format!("{size} bytes · blob {short}\n\nloading…");
        self.line_count = text.lines().count() as u16;
        self.content = PreviewContent::Text(text);
        self.numbered = false;
        self.lang = None;
        self.reset();
    }

    pub fn set_highlighted(&mut self, name: &str, lang: &str, lines: Vec<Line<'static>>) {
        self.title = sanitize::sanitize_inline(name);
        self.line_count = lines.len() as u16;
        self.content = PreviewContent::Highlighted(lines);
        self.numbered = true;
        self.lang = Some(lang.to_string());
        self.reset();
    }

    /// Move the line cursor (J/K). No-op for cursorless content
    /// (dirs, binaries, empty).
    pub fn move_cursor(&mut self, delta: i32) {
        if self.line_count == 0 {
            return;
        }
        self.cursor = self
            .cursor
            .saturating_add_signed(delta as i16)
            .min(self.line_count - 1);
    }

    /// Current cursor line, 1-based — what `␣ y` anchors to.
    pub fn line(&self) -> Option<u32> {
        (self.line_count > 0).then(|| u32::from(self.cursor) + 1)
    }

    /// Real text content supports find-in-file; dirs, binaries and
    /// meta placeholders don't.
    pub fn findable(&self) -> bool {
        self.numbered
    }

    pub fn find_active(&self) -> bool {
        self.find.is_some()
    }

    /// Open a find session (`␣ /`); the pre-find cursor is remembered
    /// for cancel.
    pub fn begin_find(&mut self) {
        if !self.findable() {
            return;
        }
        self.find = Some(FindState {
            query: String::new(),
            matches: vec![],
            current: 0,
            saved_cursor: self.cursor,
        });
    }

    /// Recompute matches on every FIND keystroke; the cursor lands on
    /// the first match at/after the session's start line (vim),
    /// wrapping to the top.
    pub fn update_find(&mut self, query: String) {
        if !self.findable() {
            return;
        }
        let saved_cursor = self
            .find
            .as_ref()
            .map(|f| f.saved_cursor)
            .unwrap_or(self.cursor);
        let matches = compute_matches(&self.plain_lines(), &query);
        let mut state = FindState {
            query,
            matches,
            current: 0,
            saved_cursor,
        };
        if !state.matches.is_empty() {
            let idx = state
                .matches
                .iter()
                .position(|m| m.line >= saved_cursor)
                .unwrap_or(0);
            state.current = idx;
            self.cursor = state.matches[idx].line;
        }
        self.find = Some(state);
    }

    /// Esc mid-session (FIND mode): restore the pre-find cursor, drop
    /// the chips — vim's cancelled `/`.
    pub fn cancel_find(&mut self) {
        if let Some(state) = self.find.take() {
            self.cursor = state.saved_cursor.min(self.line_count.saturating_sub(1));
        }
    }

    /// Esc in BROWSE with a committed find: clear the chips, keep the
    /// cursor (`:nohlsearch`).
    pub fn clear_find(&mut self) {
        self.find = None;
    }

    /// `n`/`N`: cycle matches with wraparound; the cursor follows.
    /// False when no session or no matches (key is a no-op).
    pub fn find_step(&mut self, delta: i32) -> bool {
        let Some(state) = &mut self.find else {
            return false;
        };
        if state.matches.is_empty() {
            return false;
        }
        let len = state.matches.len() as i32;
        let next = (state.current as i32 + delta).rem_euclid(len) as usize;
        state.current = next;
        self.cursor = state.matches[next].line;
        true
    }

    /// Plain text of the content (tab-expanded, matching what render
    /// shows) — the find needle haystack.
    fn plain_lines(&self) -> Vec<String> {
        match &self.content {
            PreviewContent::Text(text) => text.lines().map(|l| l.replace('\t', "    ")).collect(),
            PreviewContent::Highlighted(lines) => lines
                .iter()
                .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
                .collect(),
            _ => vec![],
        }
    }

    /// Border readout for text content: `3/41`, or `m/n · 3/41`
    /// (match of matches · line of total) while a find is active.
    fn readout(&self) -> Option<String> {
        if self.line_count == 0 {
            return None;
        }
        let pos = format!("{}/{}", self.cursor + 1, self.line_count);
        match &self.find {
            Some(f) if !f.query.is_empty() => {
                let cur = if f.matches.is_empty() {
                    0
                } else {
                    f.current + 1
                };
                Some(format!("{cur}/{} · {pos}", f.matches.len()))
            }
            _ => Some(pos),
        }
    }

    fn reset(&mut self) {
        self.scroll = 0;
        self.cursor = 0;
        self.find = None;
    }

    /// Keep the cursor inside the viewport after moves/renders.
    fn clamp_scroll(&mut self, viewport: u16) {
        if self.line_count == 0 || viewport == 0 {
            return;
        }
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + viewport {
            self.scroll = self.cursor + 1 - viewport;
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        // While finding, the title carries the query: `main.rs /ratatui`.
        let title = match &self.find {
            Some(f) => format!(" {} /{} ", self.title, f.query),
            None => format!(" {} ", self.title),
        };
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_unfocused))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(title, Style::default().fg(sem.subtext0)));
        if let Some(readout) = self.readout() {
            block = block.title_bottom(
                Line::from(Span::styled(
                    format!(" {readout} "),
                    Style::default().fg(sem.subtext0),
                ))
                .right_aligned(),
            );
        }
        // Footer: language + line count for real file content.
        if self.numbered {
            let meta = match &self.lang {
                Some(lang) => format!(" {lang} · {} lines ", self.line_count),
                None => format!(" {} lines ", self.line_count),
            };
            block = block.title_bottom(
                Line::from(Span::styled(meta, Style::default().fg(sem.overlay0))).left_aligned(),
            );
        }

        let cursored = self.line_count > 0;
        let cursor = self.cursor as usize;
        let mut lines: Vec<Line> = match &self.content {
            PreviewContent::Empty => {
                vec![Line::from(Span::styled(
                    "nothing selected",
                    Style::default().fg(sem.overlay0),
                ))]
            }
            PreviewContent::Highlighted(lines) => lines.clone(),
            PreviewContent::Text(text) => text
                .lines()
                .map(|l| {
                    Line::from(Span::styled(
                        l.replace('\t', "    "),
                        Style::default().fg(sem.text),
                    ))
                })
                .collect(),
            PreviewContent::DirSummary(children) => children
                .iter()
                .map(|e| match e.kind {
                    EntryKind::File => {
                        Line::from(Span::styled(e.name.clone(), Style::default().fg(sem.file)))
                    }
                    _ => Line::from(Span::styled(
                        format!("{}/", e.name),
                        Style::default()
                            .fg(sem.directory)
                            .add_modifier(Modifier::BOLD),
                    )),
                })
                .collect(),
            PreviewContent::Binary { size } => vec![Line::from(Span::styled(
                format!("binary file · {} bytes", size),
                Style::default().fg(sem.warning),
            ))],
        };
        // Find chips: split spans at match boundaries; matches ride the
        // search_match chip (grep-view parity), the current match the
        // warning accent so n/N reads at a glance.
        if self.numbered
            && let Some(find) = &self.find
            && !find.matches.is_empty()
        {
            let match_style = Style::default()
                .fg(sem.crust)
                .bg(sem.search_match)
                .add_modifier(Modifier::BOLD);
            let current_style = Style::default()
                .fg(sem.crust)
                .bg(sem.warning)
                .add_modifier(Modifier::BOLD);
            for (i, line) in lines.iter_mut().enumerate() {
                let ranges: Vec<(usize, usize, bool)> = find
                    .matches
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.line as usize == i)
                    .map(|(idx, m)| (m.start, m.end, idx == find.current))
                    .collect();
                if !ranges.is_empty() {
                    *line = chip_line(line, &ranges, match_style, current_style);
                }
            }
        }
        // Selection tint on the cursor line (text content only).
        if cursored && let Some(line) = lines.get_mut(cursor) {
            line.style = Style::default().bg(sem.selection_bg);
        }
        // Line-number gutter (bat/yazi parity): dim, right-aligned;
        // the cursor line's number reads bold (vim CursorLineNr).
        if self.numbered {
            let width = self.line_count.max(1).to_string().len();
            for (i, line) in lines.iter_mut().enumerate() {
                let style = if i == cursor {
                    Style::default().fg(sem.text).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(sem.overlay0)
                };
                line.spans
                    .insert(0, Span::styled(format!("{:>width$} ", i + 1), style));
            }
        }

        self.clamp_scroll(area.height.saturating_sub(2));
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .scroll((self.scroll, 0))
                .wrap(Wrap { trim: false }),
            area,
        );
        // House style: anything that scrolls shows a scrollbar.
        if self.numbered {
            super::scrollbar(
                frame,
                area,
                area.height.saturating_sub(2) as usize,
                self.line_count as usize,
                self.scroll as usize,
                theme,
            );
        }
    }
}

/// Restyle match ranges inside a rendered line: spans split at range
/// boundaries, covered segments take the chip style, everything else
/// keeps its syntax styling. `ranges` are (start, end, is_current)
/// byte offsets into the line's plain text, sorted by start.
fn chip_line(
    line: &Line<'static>,
    ranges: &[(usize, usize, bool)],
    match_style: Style,
    current_style: Style,
) -> Line<'static> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut pos = 0usize; // byte offset of the span start in the line
    for span in &line.spans {
        let span_end = pos + span.content.len();
        let mut cuts = vec![pos, span_end];
        for (start, end, _) in ranges {
            if *start > pos && *start < span_end {
                cuts.push(*start);
            }
            if *end > pos && *end < span_end {
                cuts.push(*end);
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if a == b {
                continue;
            }
            let covering = ranges.iter().find(|(s, e, _)| *s <= a && b <= *e);
            let style = match covering {
                Some((_, _, true)) => current_style,
                Some((_, _, false)) => match_style,
                None => span.style,
            };
            // Non-boundary drift on exotic unicode folds: skip the
            // segment rather than panic (cosmetic loss, no crash).
            if let Some(text) = span.content.get(a - pos..b - pos) {
                out.push(Span::styled(text.to_string(), style));
            }
        }
        pos = span_end;
    }
    let mut chipped = Line::from(out);
    chipped.style = line.style;
    chipped.alignment = line.alignment;
    chipped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_walks_and_clamps() {
        let mut p = Preview::new();
        p.set_bytes("a.rs", b"one\ntwo\nthree");
        assert_eq!(p.line(), Some(1));
        p.move_cursor(1);
        p.move_cursor(1);
        assert_eq!(p.line(), Some(3));
        p.move_cursor(1); // clamped at last line
        assert_eq!(p.line(), Some(3));
        p.move_cursor(-10); // clamped at first
        assert_eq!(p.line(), Some(1));
    }

    #[test]
    fn cursorless_content_has_no_line() {
        let mut p = Preview::new();
        p.set_dir("src", vec![]);
        assert_eq!(p.line(), None);
        p.move_cursor(1);
        assert_eq!(p.line(), None);
        p.set_bytes("blob", b"\0\0binary\0");
        assert_eq!(p.line(), None);
        assert_eq!(p.readout(), None);
    }

    #[test]
    fn cursor_resets_on_new_content() {
        let mut p = Preview::new();
        p.set_bytes("a.rs", b"one\ntwo\nthree");
        p.move_cursor(2);
        assert_eq!(p.line(), Some(3));
        p.set_highlighted("b.rs", "rust", vec![Line::from("x")]);
        assert_eq!(p.line(), Some(1));
        assert_eq!(p.readout().as_deref(), Some("1/1"));
    }

    #[test]
    fn scroll_follows_cursor_into_viewport() {
        let mut p = Preview::new();
        p.set_bytes(
            "a.rs",
            (1..=50)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n")
                .as_bytes(),
        );
        for _ in 0..49 {
            p.move_cursor(1);
        }
        p.clamp_scroll(10);
        assert_eq!(p.scroll, 40); // cursor 49 visible in rows 40..50
        p.move_cursor(-49);
        p.clamp_scroll(10);
        assert_eq!(p.scroll, 0);
    }

    fn findable_preview() -> Preview {
        let mut p = Preview::new();
        p.set_bytes(
            "main.rs",
            b"fn main() {}\nlet ratatui = 1;\nno match here\nratatui ratatui\n",
        );
        p
    }

    #[test]
    fn find_lands_on_first_match_at_or_after_cursor() {
        let mut p = findable_preview();
        p.begin_find();
        p.update_find("ratatui".into());
        assert_eq!(p.line(), Some(2)); // first match from line 1
        assert_eq!(
            p.readout().as_deref(),
            Some("1/3 · 2/4"),
            "three occurrences (two on line 4)"
        );
        // From a later cursor, the same query wraps-aware forward.
        let mut p = findable_preview();
        p.move_cursor(2); // line 3
        p.begin_find();
        p.update_find("ratatui".into());
        assert_eq!(p.line(), Some(4));
        assert_eq!(p.readout().as_deref(), Some("2/3 · 4/4"));
    }

    #[test]
    fn find_step_cycles_per_occurrence_with_wrap() {
        let mut p = findable_preview();
        p.begin_find();
        p.update_find("ratatui".into());
        assert!(p.find_step(1));
        assert_eq!(p.readout().as_deref(), Some("2/3 · 4/4"));
        assert!(p.find_step(1)); // second occurrence on line 4
        assert_eq!(p.readout().as_deref(), Some("3/3 · 4/4"));
        assert!(p.find_step(1)); // wraps to first
        assert_eq!(p.readout().as_deref(), Some("1/3 · 2/4"));
        assert!(p.find_step(-1)); // wraps back
        assert_eq!(p.readout().as_deref(), Some("3/3 · 4/4"));
    }

    #[test]
    fn find_no_match_keeps_cursor_and_shows_zero() {
        let mut p = findable_preview();
        p.move_cursor(1);
        p.begin_find();
        p.update_find("zzzz".into());
        assert_eq!(p.line(), Some(2));
        assert_eq!(p.readout().as_deref(), Some("0/0 · 2/4"));
        assert!(!p.find_step(1));
    }

    #[test]
    fn cancel_restores_cursor_clear_keeps_it() {
        let mut p = findable_preview();
        p.move_cursor(2); // line 3
        p.begin_find();
        p.update_find("ratatui".into());
        assert_eq!(p.line(), Some(4));
        p.cancel_find();
        assert_eq!(p.line(), Some(3), "cancel restores the pre-find line");
        assert!(!p.find_active());

        let mut p = findable_preview();
        p.begin_find();
        p.update_find("ratatui".into());
        p.clear_find(); // :nohlsearch
        assert_eq!(p.line(), Some(2), "clear keeps the match line");
        assert!(!p.find_active());
    }

    #[test]
    fn find_requires_text_content() {
        let mut p = Preview::new();
        p.set_dir("src", vec![]);
        assert!(!p.findable());
        p.begin_find();
        assert!(!p.find_active(), "dir summaries can't be searched");
    }

    #[test]
    fn new_content_drops_the_session() {
        let mut p = findable_preview();
        p.begin_find();
        p.update_find("ratatui".into());
        assert!(p.find_active());
        p.set_bytes("other.rs", b"fresh\n");
        assert!(!p.find_active());
        assert_eq!(p.line(), Some(1));
    }

    #[test]
    fn find_is_case_insensitive() {
        let mut p = Preview::new();
        p.set_bytes("a.rs", b"Ratatui RATATUI\n");
        p.begin_find();
        p.update_find("ratatui".into());
        assert_eq!(p.readout().as_deref(), Some("1/2 · 1/1"));
    }

    #[test]
    fn chip_line_splits_spans_at_boundaries() {
        use ratatui::style::Color;
        let line = Line::from(vec![
            Span::styled("let ".to_string(), Style::default().fg(Color::Blue)),
            Span::styled("ratatui".to_string(), Style::default().fg(Color::Green)),
        ]);
        let m = Style::default().bg(Color::Yellow);
        let c = Style::default().bg(Color::Red);
        let chipped = chip_line(&line, &[(4, 11, true)], m, c);
        assert_eq!(chipped.spans.len(), 2);
        assert_eq!(chipped.spans[0].content.as_ref(), "let ");
        assert_eq!(chipped.spans[0].style.fg, Some(Color::Blue));
        assert_eq!(chipped.spans[1].content.as_ref(), "ratatui");
        assert_eq!(chipped.spans[1].style.bg, Some(Color::Red));

        // A match spanning two spans splits both.
        let line = Line::from(vec![
            Span::raw("ab".to_string()),
            Span::raw("cd".to_string()),
        ]);
        let chipped = chip_line(&line, &[(1, 3, false)], m, c);
        let texts: Vec<&str> = chipped.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, ["a", "b", "c", "d"]);
        assert_eq!(chipped.spans[1].style.bg, Some(Color::Yellow));
        assert_eq!(chipped.spans[2].style.bg, Some(Color::Yellow));
        assert_eq!(chipped.spans[3].style.bg, None);
    }
}
