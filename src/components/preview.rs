//! Preview column: sanitized text for files, child listing for dirs.
//! Syntax highlighting lands in milestone 5; text is already sanitized.
//! Find-in-file (`␣ /`) lives in `find.rs`.

mod find;

use find::{FindState, chip_line};

use super::pane::{Entry, EntryKind};
use crate::components::modeline::fit_middle;
use crate::sanitize;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

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

/// The at-commit header context (sha, subject, author, date).
#[derive(Debug, Clone)]
pub struct BandContext {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

/// A blame run's first-line mark (plans/0016 M1c): the margin shows
/// sha + author where a commit's run starts; continuation lines carry
/// `None` and get a dim leader.
#[derive(Debug, Clone)]
pub struct BlameMark {
    pub sha: String,
    pub author: String,
}

pub struct Preview {
    pub content: PreviewContent,
    pub title: String,
    /// Drawn as the keyboard owner (focused border) — the browser
    /// never focuses its third column; the search view's expanded
    /// file pane does (plans/0012 M2).
    pub focused: bool,
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
    /// The header band (GitHub's file header): full path left; the
    /// at-commit context right when viewing history (plans/0016 M1b).
    band_path: Option<String>,
    band_context: Option<BandContext>,
    /// Visual-lines selection (vim's V, pane-local): the anchor line;
    /// the range is anchor..=cursor. `Y` copies it, `y` range-anchors
    /// the URL.
    visual_anchor: Option<u16>,
    /// Blame lens (plans/0016 M1c): one mark per logical line,
    /// `Some` at run starts. Drawn as a margin before the gutter.
    blame: Option<Vec<Option<BlameMark>>>,
    /// vim vertical motions (plans/0016 M1): the count buffer and a
    /// pending multi-key head (`g`, `z`).
    motion_count: String,
    motion_pending: Option<char>,
    /// Last rendered inner height — page motions measure against it.
    viewport: u16,
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
            focused: false,
            scroll: 0,
            cursor: 0,
            line_count: 0,
            numbered: false,
            lang: None,
            find: None,
            band_path: None,
            band_context: None,
            visual_anchor: None,
            blame: None,
            motion_count: String::new(),
            motion_pending: None,
            viewport: 0,
        }
    }

    /// Line total for blame-mark computation (plans/0016 M1c).
    pub fn text_line_count(&self) -> usize {
        self.line_count as usize
    }

    /// The blame lens: per-line marks, or None to leave it.
    pub fn set_blame(&mut self, marks: Option<Vec<Option<BlameMark>>>) {
        self.blame = marks;
    }
    /// The header band (always-on for file content): the full path
    /// left; at-commit context right when set (None restores).
    pub fn set_band(&mut self, path: Option<String>, context: Option<BandContext>) {
        self.band_path = path;
        self.band_context = context;
    }

    /// Blame lens active?
    pub fn blaming(&self) -> bool {
        self.blame.is_some()
    }
    /// vim's V (pane-local line visual): toggles the anchor at the
    /// cursor; motions extend the range. No-op on cursorless content.
    pub fn toggle_visual(&mut self) {
        if self.line_count == 0 {
            return;
        }
        self.visual_anchor = match self.visual_anchor {
            Some(_) => None,
            None => Some(self.cursor),
        };
    }

    /// Esc inside the pane: a selection clears first, the caller's
    /// exit follows. Returns true while visual stays/just cleared.
    pub fn clear_visual(&mut self) -> bool {
        self.visual_anchor.take().is_some()
    }

    /// The selected 1-based line range (start, end) while visual is
    /// on; None otherwise.
    pub fn visual_range(&self) -> Option<(u32, u32)> {
        let a = self.visual_anchor?;
        let (lo, hi) = (a.min(self.cursor), a.max(self.cursor));
        Some((u32::from(lo) + 1, u32::from(hi) + 1))
    }

    /// The pane's text content as shown (spans rejoined — tabs are
    /// display-expanded; the copy target is what's on screen).
    pub fn content_text(&self) -> Option<String> {
        match &self.content {
            PreviewContent::Text(text) => Some(text.clone()),
            PreviewContent::Highlighted(lines) => Some(
                lines
                    .iter()
                    .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
                    .collect::<Vec<String>>()
                    .join("\n"),
            ),
            _ => None,
        }
    }

    /// What `Y` copies: the visual range, else the cursor line
    /// (GitHub's copy button semantics — there is always something
    /// under the cursor). Returns (text, line_count_copied).
    pub fn copy_target(&self) -> Option<(String, usize)> {
        let lines: Vec<String> = self.content_text()?.lines().map(str::to_string).collect();
        let (lo, hi) = match self.visual_range() {
            Some((a, b)) => (a as usize, b as usize),
            None => {
                let l = self.line()? as usize;
                (l, l)
            }
        };
        let slice: Vec<String> = lines
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i + 1) >= lo && (*i + 1) <= hi)
            .map(|(_, l)| l.clone())
            .collect();
        // Linewise copies keep the trailing newline (vim register
        // semantics — pasting lands as whole lines).
        (!slice.is_empty()).then(|| {
            let mut text = slice.join("\n");
            text.push('\n');
            (text, slice.len())
        })
    }

    /// A focused preview — drawn as the keyboard owner (search view's
    /// expanded file pane, plans/0012 M2); the browser's third column
    /// stays unfocused.
    pub fn focused() -> Self {
        Preview {
            focused: true,
            ..Preview::new()
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

    /// Drop the cursor onto a 1-based line (hit expand, plans/0012
    /// M2): clamped to the content, scroll follows on the next
    /// render. No-op for cursorless content; `line = 0` (unknown
    /// anchor) keeps the top.
    pub fn set_cursor_line(&mut self, line: u32) {
        if self.line_count == 0 || line == 0 {
            return;
        }
        let target = line.saturating_sub(1).min(u32::from(self.line_count - 1));
        self.cursor = target as u16;
    }

    /// Current cursor line, 1-based — what `␣ y` anchors to.
    pub fn line(&self) -> Option<u32> {
        (self.line_count > 0).then(|| u32::from(self.cursor) + 1)
    }

    // ---- vim vertical motions (plans/0016 M1) ----
    //
    // The set: counts, j/k, gg/G, ctrl-d/u/f/b, {/} paragraphs,
    // % bracket match, zt/zz/zb view positioning. Horizontal motions
    // (f/t/w/…) are deliberately excluded — the pane is line-oriented;
    // once you're on the line, you're there.

    /// The pending count, cleared. None = no digits typed.
    fn take_count(&mut self) -> Option<usize> {
        let n = if self.motion_count.is_empty() {
            None
        } else {
            self.motion_count.parse().ok()
        };
        self.motion_count.clear();
        n
    }

    fn goto_line(&mut self, line_1based: usize) {
        let max = self.line_count as usize;
        self.cursor = (line_1based.max(1).min(max) - 1) as u16;
    }

    /// One key of the motion set. Consumed keys return true; anything
    /// else falls through to the caller's named actions. Counts and a
    /// pending head reset on any non-motion key.
    pub fn motion_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> bool {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        if self.line_count == 0 {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(c) if !ctrl && c.is_ascii_digit() && self.motion_pending.is_none() => {
                if self.motion_count.is_empty() && c == '0' {
                    return false; // 0 is a column motion — out of scope
                }
                self.motion_count.push(c);
                true
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                let n = self.take_count().unwrap_or(1) as i32;
                self.motion_pending = None;
                self.move_cursor(n);
                true
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                let n = self.take_count().unwrap_or(1) as i32;
                self.motion_pending = None;
                self.move_cursor(-n);
                true
            }
            KeyCode::Char('g') if !ctrl => {
                if self.motion_pending == Some('g') {
                    let n = self.take_count().unwrap_or(1);
                    self.motion_pending = None;
                    self.goto_line(n);
                } else {
                    self.motion_pending = Some('g');
                }
                true
            }
            KeyCode::Char('G') if !ctrl => {
                let n = self.take_count().unwrap_or(self.line_count as usize);
                self.motion_pending = None;
                self.goto_line(n);
                true
            }
            KeyCode::Char('d') if ctrl => {
                let n = (self.viewport as usize / 2).max(1) * self.take_count().unwrap_or(1);
                self.move_cursor(n as i32);
                true
            }
            KeyCode::Char('u') if ctrl => {
                let n = (self.viewport as usize / 2).max(1) * self.take_count().unwrap_or(1);
                self.move_cursor(-(n as i32));
                true
            }
            KeyCode::Char('f') if ctrl => {
                let n = (self.viewport as usize).max(1) * self.take_count().unwrap_or(1);
                self.move_cursor(n as i32);
                true
            }
            KeyCode::Char('b') if ctrl => {
                let n = (self.viewport as usize).max(1) * self.take_count().unwrap_or(1);
                self.move_cursor(-(n as i32));
                true
            }
            KeyCode::Char('{') if !ctrl => {
                self.motion_pending = None;
                self.take_count();
                let lines = self.plain_lines();
                let mut i = self.cursor as usize;
                // vim-true: off any blank, to the paragraph's first
                // line, then the blank above it.
                while i > 0 && lines[i].trim().is_empty() {
                    i -= 1;
                }
                while i > 0 && !lines[i - 1].trim().is_empty() {
                    i -= 1;
                }
                i = i.saturating_sub(1);
                self.cursor = i as u16;
                true
            }
            KeyCode::Char('}') if !ctrl => {
                self.motion_pending = None;
                self.take_count();
                let lines = self.plain_lines();
                let max = self.line_count as usize;
                let mut i = self.cursor as usize;
                while i + 1 < max && lines[i + 1].trim().is_empty() {
                    i += 1;
                }
                while i + 1 < max && !lines[i + 1].trim().is_empty() {
                    i += 1;
                }
                if i + 1 < max {
                    i += 1; // land on the blank below
                }
                self.cursor = i as u16;
                true
            }
            KeyCode::Char('%') if !ctrl => {
                self.motion_pending = None;
                self.take_count();
                let lines = self.plain_lines();
                if let Some(target) = bracket_match(&lines, self.cursor as usize) {
                    self.cursor = target as u16;
                }
                true
            }
            KeyCode::Char('z') if !ctrl => {
                match self.motion_pending {
                    Some('z') => {
                        // zz: center the cursor line.
                        self.motion_pending = None;
                        self.scroll = self
                            .cursor
                            .saturating_sub(self.viewport / 2)
                            .min(self.line_count.saturating_sub(self.viewport));
                    }
                    _ => self.motion_pending = Some('z'),
                }
                true
            }
            KeyCode::Char('t') if !ctrl && self.motion_pending == Some('z') => {
                self.motion_pending = None;
                // vim's zt pads past EOF rather than not pinning.
                self.scroll = self.cursor;
                true
            }
            KeyCode::Char('b') if !ctrl && self.motion_pending == Some('z') => {
                self.motion_pending = None;
                self.scroll = self
                    .cursor
                    .saturating_add_signed(1)
                    .saturating_sub(self.viewport);
                true
            }
            _ => {
                self.motion_count.clear();
                self.motion_pending = None;
                false
            }
        }
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
        let pos = match self.visual_range() {
            // VISUAL marker + the range — vim's -- VISUAL -- line.
            Some((lo, hi)) => format!("VISUAL {lo}-{hi} · {pos}"),
            None => pos,
        };
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
        self.band_path = None;
        self.band_context = None;
        self.visual_anchor = None;
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
            .border_type(theme.border_type())
            .border_style(Style::default().fg(if self.focused {
                sem.border_focused
            } else {
                sem.border_unfocused
            }))
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
        // Visual-lines (vim V): the range tints like the cursor line.
        if let Some((lo, hi)) = self.visual_range() {
            for i in (lo as usize - 1)..=(hi as usize - 1).min(lines.len().saturating_sub(1)) {
                if let Some(line) = lines.get_mut(i) {
                    line.style = Style::default().bg(sem.selection_bg);
                }
            }
        }
        // Line-number gutter, bat/helix style: sign column (▶ marks
        // the cursor line, tuicr-style) + space + right-aligned dim
        // numbers + a dim `│` divider before the content. The cursor
        // line's number reads bold (vim CursorLineNr).
        if self.numbered {
            let width = self.line_count.max(1).to_string().len();
            for (i, line) in lines.iter_mut().enumerate() {
                let cursor_line = i == cursor;
                let num_style = if cursor_line {
                    Style::default().fg(sem.text).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(sem.overlay0)
                };
                line.spans
                    .insert(0, Span::styled(format!("{:>width$}", i + 1), num_style));
                line.spans.insert(
                    0,
                    Span::styled(
                        if cursor_line { "▶ " } else { "  " },
                        if cursor_line {
                            Style::default()
                                .fg(sem.border_focused)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                );
                line.spans
                    .insert(2, Span::styled(" │ ", Style::default().fg(sem.overlay0)));
            }
        }

        // Blame lens (plans/0016 M1c): a margin before the gutter —
        // sha + author at each run's first line, a dim dot leader on
        // continuations (fugitive-style runs, not a per-line column).
        if self.numbered
            && let Some(blame) = &self.blame
        {
            for (i, line) in lines.iter_mut().enumerate() {
                let spans: Vec<Span<'static>> = match blame.get(i).cloned().flatten() {
                    Some(m) => vec![
                        Span::styled(m.sha, Style::default().fg(sem.warning)),
                        Span::styled(
                            // 8 cells: real names truncate, short ones pad.
                            format!(" {:<8.8}", m.author),
                            Style::default().fg(sem.subtext0),
                        ),
                        Span::styled(" │ ".to_string(), Style::default().fg(sem.overlay0)),
                    ],
                    None => vec![Span::styled(
                        // 19 cells: sha(7) + ' ' + author(8) + ' │ '.
                        "           ·    ".to_string() + " │ ",
                        Style::default().fg(sem.overlay0),
                    )],
                };
                for (j, sp) in spans.into_iter().enumerate() {
                    line.spans.insert(j, sp);
                }
            }
        }

        // The header band (GitHub's file header, plans/0016 M1b): on
        // file content one row under the border carries the full path
        // — plus the at-commit context on the right when viewing
        // history — on a surface0 strip so it reads as chrome, not
        // content.
        let band = self.numbered && self.band_path.is_some();
        let inner = block.inner(area);
        self.viewport = inner.height.saturating_sub(band as u16);
        self.clamp_scroll(self.viewport);
        frame.render_widget(block, area);
        let content_area = if band {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .split(inner);
            let path = self.band_path.clone().unwrap_or_default();
            let mut spans = vec![Span::styled(
                format!(" {path}"),
                Style::default()
                    .fg(sem.text)
                    .bg(sem.surface0)
                    .add_modifier(Modifier::BOLD),
            )];
            let left_w = 1 + UnicodeWidthStr::width(path.as_str());
            if let Some(ctx) = &self.band_context {
                // 42ec959 feat: … · author · date — sha in the accent
                // the history lens uses.
                let right = format!(
                    "{} · {} · {} · {} ",
                    ctx.sha, ctx.subject, ctx.author, ctx.date
                );
                let room = (inner.width as usize).saturating_sub(left_w);
                let right = fit_middle(&right, room);
                let pad = room.saturating_sub(UnicodeWidthStr::width(right.as_str()));
                spans.push(Span::styled(
                    " ".repeat(pad),
                    Style::default().bg(sem.surface0),
                ));
                spans.push(Span::styled(
                    right,
                    Style::default().fg(sem.warning).bg(sem.surface0),
                ));
            }
            let band_line = Line::from(spans);
            let band_w: usize = band_line.spans.iter().map(|s| s.content.width()).sum();
            let mut band_spans = band_line.spans;
            band_spans.push(Span::styled(
                " ".repeat((inner.width as usize).saturating_sub(band_w)),
                Style::default().bg(sem.surface0),
            ));
            frame.render_widget(Paragraph::new(Line::from(band_spans)), rows[0]);
            rows[1]
        } else {
            inner
        };
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((self.scroll, 0))
                .wrap(Wrap { trim: false }),
            content_area,
        );
        // House style: anything that scrolls shows a scrollbar.
        if self.numbered {
            super::scrollbar(
                frame,
                area,
                self.viewport as usize,
                self.line_count as usize,
                self.scroll as usize,
                theme,
            );
        }
    }
}

/// `%`: the first bracket on the cursor line, matched across lines
/// with nesting depth. Returns the target LINE (vertical motion only).
/// Strings/comments aren't parsed — the tree-sitter upgrade path is
/// plans/0013's grammar set.
fn bracket_match(lines: &[String], line: usize) -> Option<usize> {
    let text = lines.get(line)?;
    // No column to anchor on (vertical motion): the target is the
    // first bracket whose pair is NOT closed on this same line —
    // `fn main() {` means the brace, not the paren.
    let (col, b) = text
        .char_indices()
        .filter(|(_, c)| "(){}[]".contains(*c))
        .find(|(ci, c)| {
            if "([{".contains(*c) {
                !text[*ci + c.len_utf8()..].contains(pairs(*c))
            } else {
                // A closer whose opener sits on this line pairs
                // locally — keep scanning.
                !text[..*ci].contains(pairs(*c))
            }
        })?;
    let (open, close, forward) = match b {
        '(' | '[' | '{' => (b, pairs(b), true),
        _ => (pairs(b), b, false),
    };
    let mut depth = 1i32;
    if forward {
        let mut li = line;
        let mut skip = col + 1;
        while li < lines.len() {
            for (ci, c) in lines[li].char_indices() {
                if ci < skip {
                    continue;
                }
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some(li);
                    }
                }
            }
            skip = 0;
            li += 1;
        }
    } else {
        let mut li = line;
        let mut take_until = col;
        loop {
            for (ci, c) in lines[li].char_indices().rev() {
                if ci >= take_until {
                    continue;
                }
                if c == b {
                    depth += 1;
                } else if c == open {
                    depth -= 1;
                    if depth == 0 {
                        return Some(li);
                    }
                }
            }
            if li == 0 {
                break;
            }
            li -= 1;
            take_until = usize::MAX;
        }
    }
    None
}

/// Matching bracket pairs.
fn pairs(c: char) -> char {
    match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        ')' => '(',
        ']' => '[',
        '}' => '{',
        _ => unreachable!(),
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

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent {
            modifiers: KeyModifiers::CONTROL,
            ..key(code)
        }
    }

    /// 20 lines; blanks at 5 and 12; `fn main() {` on line 7, its `}`
    /// on line 15 (0-based 6 and 14).
    fn motion_preview() -> Preview {
        let mut lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        lines[4] = String::new();
        lines[11] = String::new();
        lines[6] = "fn main() {".into();
        lines[14] = "}".into();
        let body = lines.join("\n");
        let mut p = Preview::new();
        p.set_bytes("a.rs", body.as_bytes());
        p.viewport = 10;
        p
    }

    fn motions(p: &mut Preview, keys: &str) {
        for c in keys.chars() {
            assert!(p.motion_key(key(KeyCode::Char(c))), "motion {c}");
        }
    }

    #[test]
    fn counts_gg_g_and_pages() {
        let mut p = motion_preview();
        motions(&mut p, "3j");
        assert_eq!(p.line(), Some(4));
        motions(&mut p, "gg");
        assert_eq!(p.line(), Some(1));
        motions(&mut p, "5G");
        assert_eq!(p.line(), Some(5));
        motions(&mut p, "G");
        assert_eq!(p.line(), Some(20));
        // Half page down/up (viewport 10), full page.
        assert!(p.motion_key(ctrl(KeyCode::Char('u'))));
        assert_eq!(p.line(), Some(15));
        assert!(p.motion_key(ctrl(KeyCode::Char('d'))));
        assert_eq!(p.line(), Some(20));
        assert!(p.motion_key(ctrl(KeyCode::Char('b'))));
        assert_eq!(p.line(), Some(10));
        // A dangling count is consumed by the next motion only; a
        // non-motion key resets it.
        for c in "7".chars() {
            p.motion_key(key(KeyCode::Char(c)));
        }
        assert!(!p.motion_key(key(KeyCode::Char('x'))));
        motions(&mut p, "j");
        assert_eq!(p.line(), Some(11));
        // A pending g dies on a non-g key.
        motions(&mut p, "g");
        motions(&mut p, "j");
        assert_eq!(p.line(), Some(12));
        assert_eq!(p.motion_pending, None);
    }

    #[test]
    fn paragraphs_brackets_and_view_positioning() {
        let mut p = motion_preview();
        motions(&mut p, "8G"); // inside the second paragraph
        motions(&mut p, "{");
        assert_eq!(p.line(), Some(5)); // the blank above the paragraph
        motions(&mut p, "}");
        assert_eq!(p.line(), Some(12)); // next blank is line 12
        // % matches across the nested block.
        motions(&mut p, "7G");
        motions(&mut p, "%");
        assert_eq!(p.line(), Some(15));
        motions(&mut p, "%");
        assert_eq!(p.line(), Some(7));
        // zz centers the cursor line.
        motions(&mut p, "15G");
        motions(&mut p, "zz");
        assert_eq!(p.scroll, 9); // 14 - 10/2
        // zt / zb pin it top / bottom.
        motions(&mut p, "zt");
        assert_eq!(p.scroll, 14);
        motions(&mut p, "zb");
        assert_eq!(p.scroll, 5); // 15 - 10
    }

    #[test]
    fn motions_noop_on_cursorless_content() {
        let mut p = Preview::new();
        assert!(!p.motion_key(key(KeyCode::Char('j'))));
        assert!(!p.motion_key(key(KeyCode::Char('G'))));
    }
    #[test]
    fn visual_selects_and_copy_targets_it() {
        let mut p = motion_preview();
        // No visual: the copy target is the cursor line.
        p.move_cursor(1);
        let (text, n) = p.copy_target().unwrap();
        assert_eq!((text.as_str(), n), ("line 2\n", 1));
        assert_eq!(p.visual_range(), None);
        // v anchors; motions extend the range; Y targets it.
        p.toggle_visual();
        assert_eq!(p.visual_range(), Some((2, 2)));
        p.move_cursor(2);
        assert_eq!(p.visual_range(), Some((2, 4)));
        let (text, n) = p.copy_target().unwrap();
        assert_eq!(n, 3);
        assert!(text.starts_with("line 2") && text.ends_with("line 4\n"));
        // Motions move the cursor END of the selection (vim-true):
        // gg from line 4 leaves the anchor at line 2.
        motions(&mut p, "gg");
        assert_eq!(p.visual_range(), Some((1, 2)));
        // Esc ladder: first clear clears the selection.
        assert!(p.clear_visual());
        assert_eq!(p.visual_range(), None);
        assert!(!p.clear_visual());
    }

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
}
