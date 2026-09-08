//! Shared source-text presentation for preview search chips and diff emphasis.
//! Offsets for overlays are UTF-8 bytes; clipping uses terminal display cells.

mod search;
pub(crate) use search::LiteralSearch;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

pub(crate) struct TextColumns {
    pub offset: usize,
    pub width: usize,
}

impl TextColumns {
    pub const ALL: Self = Self {
        offset: 0,
        width: usize::MAX,
    };
}

/// Preserve syntax foregrounds while layering range styles and clipping.
/// A partly visible wide glyph occupies padding, never half a glyph or an
/// extra column. Invalid overlay boundaries cannot discard source bytes.
pub(crate) fn paint_line(
    line: &Line<'_>,
    columns: TextColumns,
    overlays: &[(Range<usize>, Style)],
) -> Line<'static> {
    let right = columns.offset.saturating_add(columns.width);
    let mut output = Vec::new();
    let mut column = 0;
    let mut byte_offset = 0;
    let mut overlay_index = 0;
    'spans: for span in &line.spans {
        for (local_byte, character) in span.content.char_indices() {
            if column >= right {
                break 'spans;
            }
            let byte = byte_offset + local_byte;
            let end_byte = byte + character.len_utf8();
            while overlays
                .get(overlay_index)
                .is_some_and(|(range, _)| range.end <= byte)
            {
                overlay_index += 1;
            }
            let mut style = span.style;
            if let Some((range, overlay)) = overlays.get(overlay_index)
                && range.start <= byte
                && end_byte <= range.end
            {
                style = style.patch(*overlay);
            }
            let cells = character.width().unwrap_or(0);
            let next_column = column + cells;
            if column >= columns.offset && next_column <= right {
                append(
                    &mut output,
                    &span.content[local_byte..local_byte + character.len_utf8()],
                    style,
                );
            } else if next_column > columns.offset {
                for _ in column.max(columns.offset)..next_column.min(right) {
                    append(&mut output, " ", style);
                }
            }
            column = next_column;
        }
        byte_offset += span.content.len();
    }
    let mut result = Line::from(output);
    result.style = line.style;
    result.alignment = line.alignment;
    result
}

fn append(output: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if let Some(last) = output.last_mut().filter(|last| last.style == style) {
        last.content.to_mut().push_str(text);
    } else {
        output.push(Span::styled(text.to_owned(), style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn clipping_and_non_boundary_overlays_never_drop_unicode_text() {
        let line = Line::from(Span::styled("a界b", Style::default().fg(Color::Blue)));
        let clipped = paint_line(
            &line,
            TextColumns {
                offset: 2,
                width: 2,
            },
            &[],
        );
        assert_eq!(clipped.to_string(), " b");
        let painted = paint_line(
            &line,
            TextColumns::ALL,
            &[(2..3, Style::default().bg(Color::Red))],
        );
        assert_eq!(painted.to_string(), "a界b");
        let painted = paint_line(
            &line,
            TextColumns::ALL,
            &[(1..4, Style::default().bg(Color::Red))],
        );
        let styled = painted
            .spans
            .iter()
            .find(|span| span.content == "界")
            .unwrap();
        assert_eq!(styled.style.fg, Some(Color::Blue));
        assert_eq!(styled.style.bg, Some(Color::Red));
    }
}
