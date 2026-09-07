//! Observe the actual cell grid in both drivers. Compact full-content runs
//! retain glyph/style/skip properties; metadata carries only their fingerprint.

use crate::app::App;
use ratatui::Frame;
use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::style::Color;
use rootle_trace::EventKind;
use serde::ser::{Serialize, SerializeTuple, Serializer};
use sha2::{Digest, Sha256};
use std::time::Instant;

thread_local! {
    static CURSOR: std::cell::Cell<Option<(u16, u16)>> = const { std::cell::Cell::new(None) };
    static GEOMETRY: std::cell::Cell<Option<ratatui::layout::Rect>> = const { std::cell::Cell::new(None) };
}

/// Record the requested hardware cursor, not a guess from logical selection.
pub fn place_cursor(frame: &mut Frame, position: (u16, u16)) {
    if rootle_trace::enabled() {
        CURSOR.with(|cursor| cursor.set(Some(position)));
    }
    frame.set_cursor_position(position);
}

pub fn draw(app: &mut App, frame: &mut Frame) {
    if !rootle_trace::enabled() {
        app.render(frame, frame.area());
        return;
    }
    CURSOR.with(|cursor| cursor.set(None));
    let area = frame.area();
    let frame_number = frame.count();
    let previous = GEOMETRY.with(|last| last.replace(Some(area)));
    if frame_number > 0
        && let Some(previous) = previous.filter(|previous| *previous != area)
    {
        // Observe the real draw geometry as well as OS resize notifications:
        // terminal backends may resize before a distinct input event is seen.
        rootle_trace::record_with(EventKind::Resize, || {
            serde_json::json!({"source":"render", "columns":area.width, "rows":area.height,
                "previous_columns":previous.width, "previous_rows":previous.height})
        });
    }
    let started = Instant::now();
    app.render(frame, area);
    let duration_us = started.elapsed().as_micros();
    let grid: &Buffer = frame.buffer_mut();
    rootle_trace::record_with(EventKind::Render, || {
        let cursor = CURSOR.with(std::cell::Cell::get);
        RenderObservation {
            frame_number,
            columns: area.width,
            rows: area.height,
            duration_us,
            cursor,
            cell_hash: cell_hash(grid),
            cells: rootle_trace::capture_content().then(|| cell_runs(grid)),
        }
    });
    app.record_trace_state("render");
}

#[derive(serde::Serialize)]
struct RenderObservation<'a> {
    frame_number: usize,
    columns: u16,
    rows: u16,
    duration_us: u128,
    cursor: Option<(u16, u16)>,
    /// SHA-256 over explicit little-endian dimensions and all rendered properties.
    cell_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cells: Option<Vec<Vec<CellRun<'a>>>>,
}

fn cell_hash(grid: &Buffer) -> String {
    let mut hash = Sha256::new();
    for dimension in [grid.area.x, grid.area.y, grid.area.width, grid.area.height] {
        hash.update(dimension.to_le_bytes());
    }
    for cell in &grid.content {
        hash.update((cell.symbol().len() as u64).to_le_bytes());
        hash.update(cell.symbol().as_bytes());
        for color in [cell.fg, cell.bg, cell.underline_color] {
            hash.update(color_code(color).to_le_bytes());
        }
        hash.update(cell.modifier.bits().to_le_bytes());
        hash.update(diff_code(cell.diff_option).to_le_bytes());
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in hash.finalize() {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 15)] as char);
    }
    encoded
}

fn cell_runs(grid: &Buffer) -> Vec<Vec<CellRun<'_>>> {
    let area = grid.area;
    (area.y..area.bottom())
        .map(|row| {
            let mut runs = Vec::new();
            let mut column = area.x;
            while column < area.right() {
                let cell = &grid[(column, row)];
                let mut end = column + 1;
                while end < area.right() && grid[(end, row)] == *cell {
                    end += 1;
                }
                runs.push(CellRun {
                    length: end - column,
                    cell,
                });
                column = end;
            }
            runs
        })
        .collect()
}

struct CellRun<'a> {
    length: u16,
    cell: &'a Cell,
}

impl Serialize for CellRun<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Schema 1: [repeat, glyph, foreground, background, underline, modifiers, diff_option].
        // Named Rust fields and a compact wire representation keep ordinary full
        // terminal captures well inside the per-record budget.
        let mut tuple = serializer.serialize_tuple(7)?;
        tuple.serialize_element(&self.length)?;
        tuple.serialize_element(self.cell.symbol())?;
        tuple.serialize_element(&color_code(self.cell.fg))?;
        tuple.serialize_element(&color_code(self.cell.bg))?;
        tuple.serialize_element(&color_code(self.cell.underline_color))?;
        tuple.serialize_element(&self.cell.modifier.bits())?;
        tuple.serialize_element(&diff_code(self.cell.diff_option))?;
        tuple.end()
    }
}

fn color_code(color: Color) -> u32 {
    match color {
        Color::Reset => 0,
        Color::Black => 1,
        Color::Red => 2,
        Color::Green => 3,
        Color::Yellow => 4,
        Color::Blue => 5,
        Color::Magenta => 6,
        Color::Cyan => 7,
        Color::Gray => 8,
        Color::DarkGray => 9,
        Color::LightRed => 10,
        Color::LightGreen => 11,
        Color::LightYellow => 12,
        Color::LightBlue => 13,
        Color::LightMagenta => 14,
        Color::LightCyan => 15,
        Color::White => 16,
        Color::Indexed(index) => 256 + u32::from(index),
        Color::Rgb(red, green, blue) => {
            (1 << 24) | (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
        }
    }
}

fn diff_code(option: CellDiffOption) -> u32 {
    match option {
        CellDiffOption::None => 0,
        CellDiffOption::Skip => 1,
        CellDiffOption::AlwaysUpdate => 2,
        CellDiffOption::ForcedWidth(width) => (1 << 16) | u32::from(width.get()),
    }
}
