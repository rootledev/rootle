//! Wrapped read-only prose inside the shared preview shell. Layout changes
//! only when content or width changes; scrolling uses the shared row viewport.

use crate::components::list_view::{ListMovement, RowLayout, ScrollMovement, Viewport};
use crate::theme::Theme;
use ratatui::{Frame, layout::Rect, style::Style, text::Line};
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

#[derive(Default)]
pub(super) struct ProseViewport {
    viewport: Viewport,
    width: Option<u16>,
    rows: Vec<Range<usize>>,
}

impl ProseViewport {
    pub fn scroll(&mut self, movement: ListMovement) {
        self.viewport.scroll(match movement {
            ListMovement::Next => ScrollMovement::Down,
            ListMovement::Previous => ScrollMovement::Up,
            ListMovement::First => ScrollMovement::Top,
            ListMovement::Last => ScrollMovement::Bottom,
        });
    }

    pub fn page(&mut self, forward: bool, half: bool) {
        let rows = self.viewport.page_rows(half) as isize;
        self.viewport
            .scroll_rows(if forward { rows } else { -rows });
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        outer: Rect,
        inner: Rect,
        text: &str,
        theme: &Theme,
    ) {
        if inner.is_empty() {
            return;
        }
        if self.width != Some(inner.width) {
            self.rows.clear();
            let mut offset = 0;
            for raw in text.split_inclusive('\n') {
                let line = raw.strip_suffix('\n').unwrap_or(raw);
                let mut start = offset;
                let mut cells = 0;
                for (byte, character) in line.char_indices() {
                    let width = character.width().unwrap_or(0);
                    if cells + width > usize::from(inner.width) && offset + byte > start {
                        self.rows.push(start..offset + byte);
                        start = offset + byte;
                        cells = 0;
                    }
                    cells += width;
                }
                self.rows.push(start..offset + line.len());
                offset += raw.len();
            }
            self.width = Some(inner.width);
        }
        let rows = &self.rows;
        self.viewport.render_virtual(
            frame,
            outer,
            inner,
            RowLayout {
                total: rows.len(),
                selected: None,
            },
            |row| {
                Line::from(&text[rows[row].clone()]).style(Style::default().fg(theme.semantic.text))
            },
            theme,
        );
    }
}
