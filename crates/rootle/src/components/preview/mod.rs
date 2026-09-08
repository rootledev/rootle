//! Preview column: sanitized text for files, child listing for dirs.
//! Find-in-file (`␣ /`) lives in `find.rs`.

mod prose;
mod render;

mod lens;
pub use lens::{BandContext, BlameMark};
mod motion;

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
    Prose(String),
    /// Syntax-highlighted lines (Tree-sitter captures → palette colors).
    Highlighted(Vec<Line<'static>>),
    DirSummary(Vec<Entry>),
    Binary {
        size: usize,
    },
}

pub struct Preview {
    pub content: PreviewContent,
    pub title: String,
    /// Drawn as the keyboard owner (focused border) — the browser
    /// never focuses its third column; the search view's expanded
    /// file pane does (plans/0012 M2).
    pub focused: bool,
    /// Vertical scroll offset (lines), follows the line cursor.
    scroll: usize,
    /// Line cursor (0-based) — `J/K` walk it, `␣ y` anchors the yank
    /// URL to it (plans/0006 §5). Only text content is cursored.
    cursor: usize,
    /// Total logical lines of the current text content; 0 = cursorless.
    line_count: usize,
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
    visual_anchor: Option<usize>,
    /// Blame lens (plans/0016 M1c): one mark per logical line,
    /// `Some` at run starts. Drawn as a margin before the gutter.
    blame: Option<Vec<Option<BlameMark>>>,
    /// vim vertical motions (plans/0016 M1): the count buffer and a
    /// pending multi-key head (`g`, `z`).
    motion_count: crate::keymap::MotionCount,
    motion_pending: Option<crate::keymap::MotionPrefix>,
    /// Last rendered inner height — page motions measure against it.
    viewport: usize,
    prose_viewport: prose::ProseViewport,
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
            motion_count: crate::keymap::MotionCount::default(),
            motion_pending: None,
            viewport: 0,
            prose_viewport: prose::ProseViewport::default(),
        }
    }

    /// Line total for blame-mark computation (plans/0016 M1c).
    pub fn text_line_count(&self) -> usize {
        self.line_count
    }

    /// The pane's text content as shown (spans rejoined — tabs are
    /// display-expanded; the copy target is what's on screen).
    pub fn content_text(&self) -> Option<String> {
        match &self.content {
            PreviewContent::Text(text) | PreviewContent::Prose(text) => Some(text.clone()),
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
            self.line_count = text.lines().count();
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
    fn set_meta_text(&mut self, name: &str, size: Option<u64>, sha: &str, tail: &str) {
        self.title = sanitize::sanitize_inline(name);
        let size = size.map(|s| s.to_string()).unwrap_or_else(|| "?".into());
        let short = &sha[..sha.len().min(7)];
        let text = format!("{size} bytes · blob {short}\n\n{tail}");
        self.line_count = text.lines().count();
        self.content = PreviewContent::Text(text);
        self.numbered = false;
        self.lang = None;
        self.reset();
    }

    pub fn set_file_meta(&mut self, name: &str, size: Option<u64>, sha: &str) {
        self.set_meta_text(name, size, sha, "loading…");
    }

    /// Fetch failure: the meta line stays, the error replaces the
    /// loading placeholder — re-shown on every re-select (0023
    /// breaker found the "loading…"-forever regression).
    pub fn set_error(&mut self, name: &str, size: Option<u64>, sha: &str, message: &str) {
        self.set_meta_text(name, size, sha, &format!("error: {message}"));
    }

    pub fn set_highlighted(&mut self, name: &str, lang: &str, lines: Vec<Line<'static>>) {
        self.title = sanitize::sanitize_inline(name);
        self.line_count = lines.len();
        self.content = PreviewContent::Highlighted(lines);
        self.numbered = true;
        self.lang = Some(lang.to_string());
        self.reset();
    }

    /// Already-sanitized prose uses the same cursor, scrolling and shell as files.
    pub(crate) fn set_text(&mut self, title: &str, text: String) {
        self.title = sanitize::sanitize_inline(title);
        self.line_count = 0;
        self.content = PreviewContent::Prose(text);
        self.numbered = false;
        self.lang = None;
        self.reset();
    }

    pub(crate) fn scroll_text(&mut self, movement: crate::components::list_view::ListMovement) {
        self.prose_viewport.scroll(movement);
    }

    pub(crate) fn page_text(&mut self, forward: bool, half: bool) {
        self.prose_viewport.page(forward, half);
    }

    pub(crate) fn pane_block(title: String, focused: bool, theme: &Theme) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(if focused {
                theme.semantic.border_focused
            } else {
                theme.semantic.border_unfocused
            }))
            .style(Style::default().bg(theme.semantic.base))
            .title(Span::styled(
                title,
                Style::default().fg(theme.semantic.subtext0),
            ))
    }

    // ---- vim vertical motions (plans/0016 M1) ----
    //
    // The set: counts, j/k, gg/G, ctrl-d/u/f/b, {/} paragraphs,
    // % bracket match, zt/zz/zb view positioning. Horizontal motions
    // (f/t/w/…) are deliberately excluded — the pane is line-oriented;
    // once you're on the line, you're there.

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

    fn reset(&mut self) {
        self.band_path = None;
        self.band_context = None;
        self.visual_anchor = None;
        self.scroll = 0;
        self.cursor = 0;
        self.find = None;
        self.prose_viewport = prose::ProseViewport::default();
    }

    /// Keep the cursor inside the viewport after moves/renders.
    fn clamp_scroll(&mut self, viewport: usize) {
        if self.line_count == 0 || viewport == 0 {
            return;
        }
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + viewport {
            self.scroll = self.cursor + 1 - viewport;
        }
    }
}

#[cfg(test)]
mod tests;
