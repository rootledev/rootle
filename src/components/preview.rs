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
        }
    }

    /// Load raw bytes (as fetched from a blob); binary → placeholder.
    pub fn set_bytes(&mut self, name: &str, bytes: &[u8]) {
        self.title = sanitize::sanitize_inline(name);
        if sanitize::is_binary(bytes) {
            self.content = PreviewContent::Binary { size: bytes.len() };
            self.line_count = 0;
        } else {
            let text = sanitize::sanitize(bytes);
            self.line_count = text.lines().count() as u16;
            self.content = PreviewContent::Text(text);
        }
        self.reset();
    }

    pub fn set_dir(&mut self, name: &str, children: Vec<Entry>) {
        self.title = format!("{}/", sanitize::sanitize_inline(name));
        self.content = PreviewContent::DirSummary(children);
        self.line_count = 0;
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
        self.reset();
    }

    pub fn set_highlighted(&mut self, name: &str, lines: Vec<Line<'static>>) {
        self.title = sanitize::sanitize_inline(name);
        self.line_count = lines.len() as u16;
        self.content = PreviewContent::Highlighted(lines);
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

    /// Border readout (`3/41`) for text content.
    fn readout(&self) -> Option<String> {
        (self.line_count > 0).then(|| format!("{}/{}", self.cursor + 1, self.line_count))
    }

    fn reset(&mut self) {
        self.scroll = 0;
        self.cursor = 0;
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
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_unfocused))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(
                format!(" {} ", self.title),
                Style::default().fg(sem.subtext0),
            ));
        if let Some(readout) = self.readout() {
            block = block.title_bottom(
                Line::from(Span::styled(
                    format!(" {readout} "),
                    Style::default().fg(sem.subtext0),
                ))
                .right_aligned(),
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
        // Selection tint on the cursor line (text content only).
        if cursored && let Some(line) = lines.get_mut(cursor) {
            line.style = Style::default().bg(sem.selection_bg);
        }

        self.clamp_scroll(area.height.saturating_sub(2));
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .scroll((self.scroll, 0))
                .wrap(Wrap { trim: false }),
            area,
        );
    }
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
        p.set_highlighted("b.rs", vec![Line::from("x")]);
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
