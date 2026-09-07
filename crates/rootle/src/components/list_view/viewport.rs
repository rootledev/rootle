//! Display-row viewport shared by lists and scroll-only text surfaces.

use crate::theme::Theme;
use ratatui::{Frame, layout::Rect, text::Line, widgets::Paragraph};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RowIndex(usize);
impl RowIndex {
    pub const fn new(row: usize) -> Self {
        Self(row)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Inclusive start, exclusive end of one selected item's displayed rows.
#[derive(Debug, Clone, Copy)]
pub struct RowSpan {
    start: RowIndex,
    end: RowIndex,
}
impl RowSpan {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: RowIndex::new(start),
            end: RowIndex::new(end.max(start.saturating_add(1))),
        }
    }
    pub fn single(row: usize) -> Self {
        Self::new(row, row.saturating_add(1))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ScrollMovement {
    Down,
    Up,
    PageDown,
    PageUp,
    Top,
    Bottom,
}

#[derive(Debug, Default)]
pub struct Viewport {
    offset: RowIndex,
    height: usize,
    total: usize,
}

pub struct RowLayout {
    pub total: usize,
    pub selected: Option<RowSpan>,
}

impl Viewport {
    pub fn offset(&self) -> RowIndex {
        self.offset
    }
    pub fn reset(&mut self) {
        self.offset = RowIndex::default();
    }

    pub fn scroll(&mut self, movement: ScrollMovement) {
        let page = self.height.max(1);
        let bottom = self.total.saturating_sub(self.height);
        self.offset.0 = match movement {
            ScrollMovement::Down => self.offset.0.saturating_add(1).min(bottom),
            ScrollMovement::Up => self.offset.0.saturating_sub(1),
            ScrollMovement::PageDown => self.offset.0.saturating_add(page).min(bottom),
            ScrollMovement::PageUp => self.offset.0.saturating_sub(page),
            ScrollMovement::Top => 0,
            ScrollMovement::Bottom => bottom,
        };
    }

    /// Keep a selected row span visible with the smallest possible scroll.
    /// A row taller than the viewport is anchored at its first line.
    pub fn layout(&mut self, height: u16, total: usize, selected: Option<RowSpan>) {
        self.height = usize::from(height);
        self.total = total;
        if height == 0 {
            self.reset();
            return;
        }
        if let Some(selected) = selected.filter(|span| span.start.0 < total) {
            let start = selected.start.0;
            let end = selected.end.0.min(total);
            if start < self.offset.0 || end.saturating_sub(start) > self.height {
                self.offset.0 = start;
            } else if end > self.offset.0.saturating_add(self.height) {
                self.offset.0 = end.saturating_sub(self.height);
            }
        }
        self.offset.0 = self.offset.0.min(total.saturating_sub(self.height));
    }

    /// Slice before rendering: Paragraph's u16 scroll field cannot represent
    /// long lists/diffs. Only the visible lines are handed to ratatui.
    pub fn render<'a>(
        &mut self,
        frame: &mut Frame,
        outer: Rect,
        inner: Rect,
        lines: Vec<Line<'a>>,
        selected: Option<RowSpan>,
        theme: &Theme,
    ) {
        self.layout(inner.height, lines.len(), selected);
        if inner.is_empty() {
            return;
        }
        let visible: Vec<_> = lines
            .into_iter()
            .skip(self.offset.0)
            .take(self.height)
            .collect();
        frame.render_widget(Paragraph::new(visible), inner);
        crate::components::scrollbar(frame, outer, self.height, self.total, self.offset.0, theme);
    }

    /// Build only viewport-visible rows for large prepared lists and diffs.
    pub fn render_virtual<'a>(
        &mut self,
        frame: &mut Frame,
        outer: Rect,
        inner: Rect,
        layout: RowLayout,
        row: impl Fn(usize) -> Line<'a>,
        theme: &Theme,
    ) {
        self.layout(inner.height, layout.total, layout.selected);
        if inner.is_empty() {
            return;
        }
        let visible: Vec<_> = (self.offset.0
            ..self.offset.0.saturating_add(self.height).min(self.total))
            .map(row)
            .collect();
        frame.render_widget(Paragraph::new(visible), inner);
        crate::components::scrollbar(frame, outer, self.height, self.total, self.offset.0, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_and_multiline_items_use_display_spans_not_item_indices() {
        let mut view = Viewport::default();
        view.layout(4, 12, Some(RowSpan::new(7, 9)));
        assert_eq!(view.offset().get(), 5);
        view.layout(4, 12, Some(RowSpan::new(6, 8)));
        assert_eq!(
            view.offset().get(),
            5,
            "moving up inside viewport must not jump"
        );
        view.layout(2, 12, Some(RowSpan::new(6, 8)));
        assert_eq!(view.offset().get(), 6);
        view.layout(1, 12, Some(RowSpan::new(6, 8)));
        assert_eq!(view.offset().get(), 6);
        view.layout(0, 12, Some(RowSpan::new(6, 8)));
        assert_eq!(view.offset().get(), 0);
    }

    #[test]
    fn long_content_does_not_wrap_the_scroll_offset() {
        let mut view = Viewport::default();
        view.layout(20, 100_000, Some(RowSpan::single(90_000)));
        assert_eq!(view.offset().get(), 89_981);
        view.layout(20, 3, None);
        assert_eq!(view.offset().get(), 0);
    }
}
