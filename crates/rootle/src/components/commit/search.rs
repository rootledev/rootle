//! A diff owns its transient find session. Queries index source rows only;
//! cancellation restores the prior query, cursor and horizontal viewport.

use super::prepare::PatchRow;
use crate::components::list_view::RowIndex;
use crate::components::text::LiteralSearch;
use crate::components::vim_input::{Outcome, VimInput};
use crate::theme::Theme;
use ratatui::{
    crossterm::event::KeyEvent,
    style::{Modifier, Style},
};
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy)]
pub(super) struct DiffPosition {
    pub row: RowIndex,
    pub horizontal: usize,
}

struct DiffMatch {
    row: RowIndex,
    bytes: Range<usize>,
    columns: Range<usize>,
}

struct SearchBaseline {
    query: String,
    current: Option<usize>,
    position: DiffPosition,
}

pub(super) struct DiffSearch {
    input: VimInput,
    query: String,
    matches: Vec<DiffMatch>,
    current: Option<usize>,
    baseline: Option<SearchBaseline>,
}

impl Default for DiffSearch {
    fn default() -> Self {
        Self {
            input: VimInput::transient(),
            query: String::new(),
            matches: Vec::new(),
            current: None,
            baseline: None,
        }
    }
}

impl DiffSearch {
    pub fn editing(&self) -> bool {
        self.baseline.is_some()
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn cursor_column(&self) -> usize {
        self.query
            .chars()
            .take(self.input.cursor())
            .map(|character| unicode_width::UnicodeWidthChar::width(character).unwrap_or(0))
            .sum()
    }
    pub fn readout(&self) -> String {
        format!(
            "{}/{}",
            self.current.map_or(0, |index| index + 1),
            self.matches.len()
        )
    }

    pub fn begin(&mut self, position: DiffPosition) {
        self.baseline = Some(SearchBaseline {
            query: self.query.clone(),
            current: self.current,
            position,
        });
        self.input.prefill(&self.query);
    }

    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        rows: &[PatchRow],
        position: DiffPosition,
        width: usize,
    ) -> Option<DiffPosition> {
        match self.input.handle_key(key) {
            Outcome::Changed => {
                self.query = self.input.value();
                let start = self
                    .baseline
                    .as_ref()
                    .map_or(position.row, |baseline| baseline.position.row);
                self.recompute(rows, start);
                self.target(position, width)
            }
            Outcome::Submitted => {
                self.baseline = None;
                None
            }
            Outcome::Cancelled => {
                let baseline = self.baseline.take()?;
                self.query = baseline.query;
                self.input.set(&self.query);
                self.recompute(rows, baseline.position.row);
                self.current = baseline.current.filter(|index| *index < self.matches.len());
                Some(baseline.position)
            }
            Outcome::Noop => None,
        }
    }

    pub fn clear(&mut self) -> bool {
        let existed = !self.query.is_empty() || self.editing();
        self.query.clear();
        self.input.set("");
        self.matches.clear();
        self.current = None;
        self.baseline = None;
        existed
    }

    pub fn step(
        &mut self,
        forward: bool,
        position: DiffPosition,
        width: usize,
    ) -> Option<DiffPosition> {
        if self.matches.is_empty() {
            return None;
        }
        let current = self.current.unwrap_or(0);
        self.current = Some(if forward {
            if current + 1 == self.matches.len() {
                0
            } else {
                current + 1
            }
        } else if current == 0 {
            self.matches.len() - 1
        } else {
            current - 1
        });
        self.target(position, width)
    }

    fn target(&self, position: DiffPosition, width: usize) -> Option<DiffPosition> {
        let found = &self.matches[self.current?];
        let width = width.max(1);
        let horizontal =
            if found.columns.start < position.horizontal || found.columns.len() >= width {
                found.columns.start
            } else if found.columns.end > position.horizontal.saturating_add(width) {
                found.columns.end.saturating_sub(width)
            } else {
                position.horizontal
            };
        Some(DiffPosition {
            row: found.row,
            horizontal,
        })
    }

    fn recompute(&mut self, rows: &[PatchRow], start: RowIndex) {
        self.matches.clear();
        if !self.query.is_empty() {
            let search = LiteralSearch::new(&self.query);
            for (row, content) in rows.iter().enumerate() {
                let PatchRow::Content(content) = content else {
                    continue;
                };
                let text = content.syntax.to_string();
                let mut byte = 0;
                let mut column = 0;
                for bytes in search.ranges(&text) {
                    column += text[byte..bytes.start].width();
                    let end_column = column + text[bytes.clone()].width();
                    byte = bytes.end;
                    self.matches.push(DiffMatch {
                        row: RowIndex::new(row),
                        bytes,
                        columns: column..end_column,
                    });
                    column = end_column;
                }
            }
        }
        self.current = (!self.matches.is_empty()).then(|| {
            self.matches
                .iter()
                .position(|found| found.row >= start)
                .unwrap_or(0)
        });
    }

    pub fn overlays(&self, row: RowIndex, theme: &Theme) -> Vec<(Range<usize>, Style)> {
        let start = self.matches.partition_point(|found| found.row < row);
        self.matches[start..]
            .iter()
            .take_while(|found| found.row == row)
            .enumerate()
            .map(|(offset, found)| {
                let current = self.current == Some(start + offset);
                let style = Style::default()
                    .fg(theme.semantic.crust)
                    .bg(if current {
                        theme.semantic.warning
                    } else {
                        theme.semantic.search_match
                    })
                    .add_modifier(if current {
                        Modifier::BOLD | Modifier::UNDERLINED
                    } else {
                        Modifier::BOLD
                    });
                (found.bytes.clone(), style)
            })
            .collect()
    }
}

impl super::OpenDelta {
    fn position(&self) -> DiffPosition {
        DiffPosition {
            row: RowIndex::new(self.selection.selected().get()),
            horizontal: self.horizontal,
        }
    }
    fn apply_position(&mut self, position: DiffPosition) {
        self.selection
            .select(crate::components::list_view::ItemIndex::new(
                position.row.get(),
            ));
        self.horizontal = position.horizontal;
    }
}

impl super::CommitView {
    pub(crate) fn searching(&self) -> bool {
        self.delta
            .as_ref()
            .is_some_and(|delta| delta.search.editing())
    }

    pub(super) fn begin_diff_search(&mut self) {
        if self.focus == super::CommitFocus::Preview
            && let Some(delta) = &mut self.delta
        {
            delta.search.begin(delta.position());
        }
    }

    pub(super) fn diff_search_key(&mut self, key: KeyEvent) {
        let super::CommitLoad::Ready(content) = &self.load else {
            return;
        };
        if let Some(delta) = &mut self.delta {
            let rows = &content.files[delta.file.get()]
                .prepared
                .as_ref()
                .expect("opened diff is prepared")
                .rows;
            if let Some(position) =
                delta
                    .search
                    .handle_key(key, rows, delta.position(), delta.text_width)
            {
                delta.apply_position(position);
            }
        }
    }

    pub(super) fn step_diff_match(&mut self, forward: bool) {
        if self.focus == super::CommitFocus::Preview
            && let Some(delta) = &mut self.delta
            && let Some(position) = delta
                .search
                .step(forward, delta.position(), delta.text_width)
        {
            delta.apply_position(position);
        }
    }

    pub(super) fn page(&mut self, forward: bool, half: bool) {
        if self.focus != super::CommitFocus::Preview {
            return;
        }
        if let Some(delta) = &mut self.delta {
            let super::CommitLoad::Ready(content) = &self.load else {
                return;
            };
            let total = content.files[delta.file.get()]
                .prepared
                .as_ref()
                .map_or(0, |patch| patch.rows.len());
            let step = delta.viewport.page_rows(half);
            let current = delta.selection.selected().get();
            let next = if forward {
                current.saturating_add(step).min(total.saturating_sub(1))
            } else {
                current.saturating_sub(step)
            };
            delta
                .selection
                .select(crate::components::list_view::ItemIndex::new(next));
        } else {
            self.message.page_text(forward, half);
        }
    }
}
