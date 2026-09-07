//! Shared selection, filtering and display-row scrolling for scanned lists.
//! Components build their own rows; this module owns navigation semantics.
//! An item index is never a display-row offset: headings and multi-line rows
//! are represented explicitly by `RowSpan` when presenting a list.

mod filter;
mod viewport;

pub use filter::{FilterOutcome, ListFilter};
pub use viewport::{RowIndex, RowLayout, RowSpan, ScrollMovement, Viewport};

use crate::theme::Semantic;
use ratatui::style::Style;
use ratatui::text::Span;

/// An index into a component's visible item projection, not its row layout.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemIndex(usize);

impl ItemIndex {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMovement {
    Next,
    Previous,
    First,
    Last,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    Clamp,
    Wrap,
}

/// Selection only. The viewport owns display rows independently.
#[derive(Debug, Default)]
pub struct ListCursor {
    selected: ItemIndex,
}

impl ListCursor {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn selected(&self) -> ItemIndex {
        self.selected
    }
    pub fn select(&mut self, selected: ItemIndex) {
        self.selected = selected;
    }
    pub fn reset(&mut self) {
        self.selected = ItemIndex::default();
    }

    pub fn clamp(&mut self, item_count: usize) {
        self.selected.0 = self.selected.0.min(item_count.saturating_sub(1));
    }

    pub fn advance(&mut self, movement: ListMovement, item_count: usize, boundary: Boundary) {
        if item_count == 0 {
            self.reset();
            return;
        }
        self.clamp(item_count);
        let last = item_count - 1;
        self.selected.0 = match movement {
            ListMovement::First => 0,
            ListMovement::Last => last,
            ListMovement::Next if self.selected.0 < last => self.selected.0 + 1,
            ListMovement::Previous if self.selected.0 > 0 => self.selected.0 - 1,
            ListMovement::Next if boundary == Boundary::Wrap => 0,
            ListMovement::Previous if boundary == Boundary::Wrap => last,
            _ => self.selected.0,
        };
    }

    /// Restore identity after a refreshed/filter-projected list changed order.
    /// Callers supply keys, not captions or transient display-line positions.
    pub fn restore<Key: PartialEq>(&mut self, keys: &[Key], selected_key: Option<&Key>) {
        if let Some(position) =
            selected_key.and_then(|key| keys.iter().position(|item| item == key))
        {
            self.select(ItemIndex::new(position));
        } else {
            self.clamp(keys.len());
        }
    }
}

/// Shared selection chrome; callers retain content-specific colors on spans.
pub fn selection_style(selected: bool, semantic: &Semantic) -> Style {
    if selected {
        Style::default()
            .fg(semantic.selection_fg)
            .bg(semantic.selection_bg)
    } else {
        Style::default().fg(semantic.text)
    }
}

pub fn selection_gutter(selected: bool, semantic: &Semantic) -> Span<'static> {
    Span::styled(
        if selected { "▌ " } else { "  " },
        Style::default()
            .fg(semantic.border_focused)
            .patch(if selected {
                selection_style(true, semantic)
            } else {
                Style::default()
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_clamps_wraps_and_recovers_from_empty_results() {
        let mut cursor = ListCursor::new();
        cursor.advance(ListMovement::Previous, 3, Boundary::Wrap);
        assert_eq!(cursor.selected().get(), 2);
        cursor.advance(ListMovement::Next, 3, Boundary::Clamp);
        assert_eq!(cursor.selected().get(), 2);
        cursor.advance(ListMovement::Next, 3, Boundary::Wrap);
        assert_eq!(cursor.selected().get(), 0);
        cursor.advance(ListMovement::Last, 0, Boundary::Wrap);
        assert_eq!(cursor.selected().get(), 0);
    }

    #[test]
    fn refresh_preserves_item_identity_not_old_position() {
        let mut cursor = ListCursor::new();
        cursor.select(ItemIndex::new(1));
        cursor.restore(&["c", "a", "b"], Some(&"b"));
        assert_eq!(cursor.selected().get(), 2);
        cursor.restore(&["a"], Some(&"b"));
        assert_eq!(cursor.selected().get(), 0);
    }
}
