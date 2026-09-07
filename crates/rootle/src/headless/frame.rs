//! Cell-grid evidence must skip continuation cells occupied by wide glyphs.

use ratatui::buffer::Buffer;
use unicode_width::UnicodeWidthStr;

pub fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut text = String::new();
    for row in area.y..area.bottom() {
        let mut column = area.x;
        let start = text.len();
        while column < area.right() {
            let symbol = buffer[(column, row)].symbol();
            text.push_str(symbol);
            let cells = u16::try_from(symbol.width().max(1)).unwrap_or(u16::MAX);
            column = column.saturating_add(cells);
        }
        let trimmed = text[start..].trim_end().len();
        text.truncate(start + trimmed);
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{layout::Rect, style::Style};

    #[test]
    fn wide_symbols_do_not_gain_fake_spaces() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
        buffer.set_string(0, 0, "文字(hello)", Style::default());
        assert_eq!(buffer_text(&buffer), "文字(hello)\n");
    }
}
