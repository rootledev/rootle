//! Find-in-file (`␣ /`, plans/0007 §3): the session state, the
//! occurrence scan, and the chip pass that restyles matched ranges
//! inside rendered lines. Chips render from `matches`; `n`/`N` move
//! `current` and the line cursor follows.

use super::Preview;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// One find occurrence: 0-based line + byte range in that line's
/// plain (tab-expanded) text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FindMatch {
    pub(super) line: u16,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// Active find-in-file session (plans/0007 §3). Chips render from
/// `matches`; `n`/`N` move `current` and the line cursor follows.
pub(super) struct FindState {
    pub(super) query: String,
    pub(super) matches: Vec<FindMatch>,
    /// Index into `matches` the cursor sits on.
    pub(super) current: usize,
    /// Cursor line before the session — restored on cancel (vim `/`).
    pub(super) saved_cursor: u16,
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

impl Preview {
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
}

/// Restyle match ranges inside a rendered line: spans split at range
/// boundaries, covered segments take the chip style, everything else
/// keeps its syntax styling. `ranges` are (start, end, is_current)
/// byte offsets into the line's plain text, sorted by start.
pub(super) fn chip_line(
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
