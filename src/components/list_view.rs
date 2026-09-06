//! The shared list-cursor engine (plans/0026): visible-index
//! filtering, cursor clamping, keep-visible scroll math, and the
//! selection gutter — the machinery every scrollable list in rootle
//! hand-rolled before this. The `/` filter *session* (VimInput,
//! commit/restore semantics) stays with the owning component — this
//! engine renders the result of a filter, it does not own one.

use crate::theme::Semantic;
use ratatui::style::Style;
use ratatui::text::Span;

#[derive(Debug, Default)]
pub struct ListCursor {
    /// Index into the visible (filtered) rows.
    pub cursor: usize,
    /// First visible display row (scroll offset, in rows).
    pub scroll: u16,
}

impl ListCursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep the cursor inside `0..visible_len` (empty lists park at 0).
    pub fn clamp(&mut self, visible_len: usize) {
        self.cursor = self.cursor.min(visible_len.saturating_sub(1));
    }

    /// Move by `delta` rows, clamped; empty lists stay parked.
    pub fn r#move(&mut self, visible_len: usize, delta: isize) {
        if visible_len == 0 {
            self.cursor = 0;
            return;
        }
        let next = self.cursor as isize + delta;
        self.cursor = next.clamp(0, visible_len as isize - 1) as usize;
    }

    /// The keep-visible scroll: the cursor row must sit inside the
    /// viewport, minimally moving the offset (the snippet this file
    /// exists to stop duplicating). `height` is viewport rows,
    /// `total` the full row count; both in the same row unit.
    pub fn keep_visible(&mut self, height: usize, total: usize) {
        let height = height.max(1);
        let max_scroll = total.saturating_sub(height);
        self.scroll = self.cursor.saturating_sub(height - 1).min(max_scroll) as u16;
    }

    /// The selection gutter span for a row (house style: `▌` on the
    /// cursor row inside `selection_bg`, blank otherwise — a separate
    /// span so content styles never flip).
    pub fn gutter(&self, row: usize, sem: &Semantic) -> Span<'static> {
        if row == self.cursor {
            Span::styled(
                "▌",
                Style::default().fg(sem.selection_fg).bg(sem.selection_bg),
            )
        } else {
            Span::raw(" ")
        }
    }

    /// The cursor row's whole-line style (fg/bg per house style).
    pub fn selection_style(&self, row: usize, sem: &Semantic) -> Style {
        if row == self.cursor {
            Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
        } else {
            Style::default()
        }
    }
}

/// Filtered indices: positions in `items` whose `text_of` matches the
/// (lowercased) needle — the substring contract every `/` session
/// uses. Empty needle = everything visible.
pub fn visible_indices<T>(items: &[T], needle: &str, text_of: impl Fn(&T) -> String) -> Vec<usize> {
    let needle = needle.to_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| needle.is_empty() || text_of(item).to_lowercase().contains(&needle))
        .map(|(i, _)| i)
        .collect()
}

/// The theme's dim text style, for non-content rows.
pub fn dim(sem: &Semantic) -> Style {
    Style::default().fg(sem.subtext0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_clamps_and_parks() {
        let mut c = ListCursor::new();
        c.r#move(5, 3);
        assert_eq!(c.cursor, 3);
        c.r#move(5, -10);
        assert_eq!(c.cursor, 0);
        c.r#move(5, 99);
        assert_eq!(c.cursor, 4);
        c.r#move(0, 5);
        assert_eq!(c.cursor, 0);
    }

    #[test]
    fn keep_visible_scrolls_minimally() {
        let mut c = ListCursor::new();
        c.cursor = 9;
        c.keep_visible(4, 20);
        assert_eq!(c.scroll, 6, "cursor at bottom edge: 9-(4-1)");
        c.cursor = 1;
        c.keep_visible(4, 20);
        assert_eq!(c.scroll, 0, "near top: no scroll");
        c.cursor = 19;
        c.keep_visible(4, 20);
        assert_eq!(c.scroll, 16, "clamped to max_scroll");
        c.cursor = 2;
        c.keep_visible(10, 3);
        assert_eq!(c.scroll, 0, "content fits: no scroll ever");
    }

    #[test]
    fn filter_matches_substring_case_insensitively() {
        let items = ["src/Main.rs", "README", "lib.rs"];
        let vis = visible_indices(&items, "read", |s| s.to_string());
        assert_eq!(vis, vec![1]);
        let all = visible_indices(&items, "", |s| s.to_string());
        assert_eq!(all, vec![0, 1, 2]);
    }
}
