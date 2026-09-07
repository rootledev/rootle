//! Layout over prepared data. Only viewport normalization is mutable here.

use super::{CommitLoad, CommitView, DetailFocus, rows};
use crate::components::list_view::{RowLayout, RowSpan};
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use unicode_width::UnicodeWidthChar;

const METADATA_ROWS: u16 = 1;
const FILE_HEADER_ROWS: u16 = 1;
const MINIMUM_FILE_ROWS: u16 = 3;

pub(super) fn draw(view: &mut CommitView, frame: &mut Frame, area: Rect, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    let semantic = &theme.semantic;
    let title = match (&view.load, &view.delta) {
        (CommitLoad::Ready(content), Some(delta)) => format!(
            " {} · {}/{} ",
            content.files[delta.file.get()].label,
            delta.file.get() + 1,
            content.files.len()
        ),
        _ if view.filter.active() || !view.filter.is_empty() => {
            format!(" commit {} /{} ", view.sha_short(), view.filter.text())
        }
        _ => format!(" commit {} ", view.sha_short()),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type())
        .border_style(Style::default().fg(semantic.border_focused))
        .style(Style::default().bg(semantic.base))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }
    let content = match &view.load {
        CommitLoad::Loading => {
            frame.render_widget(Paragraph::new(" loading commit…"), inner);
            return;
        }
        CommitLoad::Failed(message) => {
            frame.render_widget(
                Paragraph::new(message.as_str()).style(Style::default().fg(semantic.error)),
                inner,
            );
            return;
        }
        CommitLoad::Ready(content) => content,
    };
    let statistics = content.detail.line_stats();
    let metadata = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        METADATA_ROWS.min(inner.height),
    );
    frame.render_widget(
        rows::statistics_row(
            &format!(
                " {} · {} · {}",
                view.sha_short(),
                content.author,
                content.date
            ),
            statistics.additions,
            statistics.deletions,
            usize::from(inner.width),
            semantic,
        ),
        metadata,
    );
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(metadata.height),
        inner.width,
        inner.height.saturating_sub(metadata.height),
    );
    if body.is_empty() {
        return;
    }

    if let Some(delta) = &mut view.delta {
        let prepared = content.files[delta.file.get()]
            .prepared
            .as_ref()
            .expect("opening a delta prepares it");
        let selected = delta.selection.selected().get();
        let layout = RowLayout {
            total: prepared.rows.len(),
            selected: Some(RowSpan::single(selected)),
        };
        delta.viewport.render_virtual(
            frame,
            area,
            body,
            layout,
            |row| {
                rows::patch_row(
                    &prepared.rows[row],
                    row == selected,
                    prepared.number_width,
                    delta.horizontal,
                    usize::from(body.width),
                    semantic,
                )
            },
            theme,
        );
        return;
    }

    let message_rows = wrap_message(&content.message, usize::from(body.width));
    let message_height = if view.focus == DetailFocus::Message {
        body.height
            .saturating_sub(FILE_HEADER_ROWS)
            .max(1)
            .min(body.height)
    } else {
        (body.height / 3)
            .min(body.height.saturating_sub(MINIMUM_FILE_ROWS))
            .min(u16::try_from(message_rows.len()).unwrap_or(u16::MAX))
    };
    let message_area = Rect::new(body.x, body.y, body.width, message_height);
    // This subpane has its own track; the file list retains the outer border.
    if !message_area.is_empty() {
        let lines = message_rows
            .into_iter()
            .map(|text| Line::from(Span::styled(text, Style::default().fg(semantic.text))))
            .collect();
        view.message_viewport
            .render(frame, message_area, message_area, lines, None, theme);
    }
    let header_y = body.y.saturating_add(message_height);
    let remaining = body.height.saturating_sub(message_height);
    if remaining == 0 {
        return;
    }
    let visible = view.visible_files(content);
    let truncated = if content.detail.truncated {
        " · truncated by provider"
    } else {
        ""
    };
    let focus = if view.focus == DetailFocus::Message {
        "message"
    } else {
        "files"
    };
    frame.render_widget(
        Paragraph::new(format!(
            " files ({}){truncated} · tab: {focus}",
            visible.len()
        ))
        .style(
            Style::default()
                .fg(semantic.subtext0)
                .bg(semantic.diff_band),
        ),
        Rect::new(body.x, header_y, body.width, FILE_HEADER_ROWS),
    );
    let files_area = Rect::new(
        body.x,
        header_y.saturating_add(FILE_HEADER_ROWS),
        body.width,
        remaining.saturating_sub(FILE_HEADER_ROWS),
    );
    if visible.is_empty() {
        frame.render_widget(Paragraph::new(" no matching files"), files_area);
        return;
    }
    let selected = view.selection.selected().get();
    let layout = RowLayout {
        total: visible.len(),
        selected: Some(RowSpan::single(selected)),
    };
    view.viewport.render_virtual(
        frame,
        area,
        files_area,
        layout,
        |row| {
            rows::file_row(
                &content.files[visible[row]],
                row == selected && view.focus == DetailFocus::Files,
                usize::from(files_area.width),
                semantic,
            )
        },
        theme,
    );
}

/// Borrowed wrapping keeps every message byte reachable without copying
/// the message or truncating it to a fixed number of lines.
fn wrap_message(lines: &[String], width: usize) -> Vec<&str> {
    if width == 0 {
        return Vec::new();
    }
    let mut wrapped = Vec::new();
    for line in lines {
        if line.is_empty() {
            wrapped.push("");
            continue;
        }
        let mut start = 0;
        let mut cells = 0;
        for (index, character) in line.char_indices() {
            let char_width = character.width().unwrap_or(0);
            if cells + char_width > width && index > start {
                wrapped.push(&line[start..index]);
                start = index;
                cells = 0;
            }
            cells += char_width;
        }
        wrapped.push(&line[start..]);
    }
    wrapped
}
