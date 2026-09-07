//! Consume nested highlight events without buffering a second copy of the file.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use tree_sitter_highlight::HighlightEvent;

use super::styles::StyleTable;

struct Flat {
    start: usize,
    end: usize,
    slot: Option<usize>,
}

pub(super) fn plain(text: &str, styles: &StyleTable) -> Vec<Line<'static>> {
    text.lines()
        .map(|line| {
            Line::from(Span::styled(
                line.replace('\t', "    "),
                styles.default_style(),
            ))
        })
        .collect()
}

/// Source events are ordered, disjoint byte ranges. Keep only the current
/// range and capture stack; a multiline range can span several output lines.
pub(super) fn styled(
    text: &str,
    events: impl Iterator<Item = HighlightEvent>,
    styles: &StyleTable,
) -> Vec<Line<'static>> {
    let mut ranges = flatten(events).peekable();
    let mut out = Vec::new();
    for (line_start, line_end) in line_ranges(text) {
        while ranges.peek().is_some_and(|range| range.end <= line_start) {
            ranges.next();
        }
        let mut spans = Vec::new();
        let mut covered = line_start;
        while let Some(range) = ranges.peek().filter(|range| range.start < line_end) {
            let start = range.start.max(line_start);
            let end = range.end.min(line_end);
            if start > covered {
                append(&mut spans, &text[covered..start], styles.default_style());
            }
            if end > start {
                // Tree-sitter works on this exact UTF-8 buffer. Invalid byte
                // boundaries must not replace source characters with lossy text.
                let Some(piece) = text.get(start..end) else {
                    return plain(text, styles);
                };
                append(&mut spans, piece, styles.style(range.slot));
                covered = end;
            }
            if range.end > line_end {
                break;
            }
            ranges.next();
        }
        if covered < line_end {
            append(&mut spans, &text[covered..line_end], styles.default_style());
        }
        out.push(Line::from(spans));
    }
    out
}

/// Merge adjacent equal styles and expand tabs directly into the owned span.
fn append(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if spans.last().is_none_or(|span| span.style != style) {
        spans.push(Span::styled(String::with_capacity(text.len()), style));
    }
    let target = spans
        .last_mut()
        .expect("span just created")
        .content
        .to_mut();
    let mut parts = text.split('\t');
    target.push_str(parts.next().unwrap_or_default());
    for part in parts {
        target.push_str("    ");
        target.push_str(part);
    }
}

fn flatten(mut events: impl Iterator<Item = HighlightEvent>) -> impl Iterator<Item = Flat> {
    let mut stack = Vec::new();
    std::iter::from_fn(move || {
        for event in events.by_ref() {
            match event {
                HighlightEvent::HighlightStart(highlight) => stack.push(highlight.0),
                HighlightEvent::HighlightEnd => {
                    stack.pop();
                }
                HighlightEvent::Source { start, end } if end > start => {
                    return Some(Flat {
                        start,
                        end,
                        slot: stack.last().copied(),
                    });
                }
                HighlightEvent::Source { .. } => {}
            }
        }
        None
    })
}

/// Exactly str::lines() semantics: CRLF strips its CR, a lone CR stays,
/// and a final newline does not introduce an extra line.
fn line_ranges(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    text.split_inclusive('\n').scan(0, |offset, line| {
        let start = *offset;
        *offset += line.len();
        let content = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(line);
        Some((start, start + content.len()))
    })
}
