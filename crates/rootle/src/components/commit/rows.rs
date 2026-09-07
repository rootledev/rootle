//! Theme-aware rows over already-sanitized commit content.

use super::prepare::{FilePresentation, PatchRow, PreparedLine};
use crate::components::pane::fit;
use crate::components::text::{TextColumns, paint_line};
use crate::theme::Semantic;
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use rootle_diff::LineOrigin;
use unicode_width::UnicodeWidthStr;

pub(super) fn statistics_row(
    label: &str,
    additions: Option<u64>,
    deletions: Option<u64>,
    width: usize,
    semantic: &Semantic,
) -> Line<'static> {
    let added = format!("+{}", count(additions));
    let deleted = format!("-{}", count(deletions));
    let suffix_width = added.width() + deleted.width() + 3;
    let label_width = width.saturating_sub(suffix_width);
    let label = fit(label, label_width);
    let padding = label_width.saturating_sub(label.width());
    Line::from(vec![
        Span::styled(
            format!("{label}{}", " ".repeat(padding)),
            Style::default().fg(semantic.text),
        ),
        Span::styled(
            format!(" {added}"),
            Style::default().fg(semantic.diff_add_fg),
        ),
        Span::styled(
            format!(" {deleted} "),
            Style::default().fg(semantic.diff_del_fg),
        ),
    ])
    .style(Style::default().bg(semantic.diff_band))
}

pub(super) fn file_row(
    file: &FilePresentation,
    selected: bool,
    width: usize,
    semantic: &Semantic,
) -> Line<'static> {
    let marker = match file.status {
        rootle_provider::FileStatus::Added => "+",
        rootle_provider::FileStatus::Removed => "-",
        rootle_provider::FileStatus::Renamed => "→",
        rootle_provider::FileStatus::Modified => "~",
    };
    let label = match &file.previous_label {
        Some(previous) => format!("{previous} → {}", file.label),
        None => file.label.clone(),
    };
    let mut line = statistics_row(
        &format!("{marker} {label}"),
        file.additions.map(u64::from),
        file.deletions.map(u64::from),
        width.saturating_sub(2),
        semantic,
    );
    line.spans.insert(
        0,
        crate::components::list_view::selection_gutter(selected, semantic),
    );
    line.style =
        crate::components::list_view::selection_style(selected, semantic).bg(if selected {
            semantic.selection_bg
        } else {
            semantic.base
        });
    if selected && let Some(label) = line.spans.get_mut(1) {
        label.style = label.style.fg(semantic.selection_fg);
    }
    line
}

pub(super) fn patch_row(
    row: &PatchRow,
    selected: bool,
    number_width: usize,
    horizontal: usize,
    width: usize,
    semantic: &Semantic,
) -> Line<'static> {
    match row {
        PatchRow::Hunk(header) => Line::from(Span::styled(
            fit(
                &format!("{} {header}", if selected { "▸" } else { " " }),
                width,
            ),
            Style::default().fg(semantic.subtext0),
        ))
        .style(Style::default().bg(semantic.diff_band)),
        PatchRow::Note(note) => Line::from(Span::styled(
            fit(
                &format!("{} {note}", if selected { "▸" } else { " " }),
                width,
            ),
            Style::default().fg(semantic.subtext0),
        )),
        PatchRow::Content(line) => {
            content_row(line, selected, number_width, horizontal, width, semantic)
        }
    }
}

fn content_row(
    line: &PreparedLine,
    selected: bool,
    digits: usize,
    horizontal: usize,
    width: usize,
    semantic: &Semantic,
) -> Line<'static> {
    let (foreground, background, emphasis) = match line.origin {
        LineOrigin::Addition => (
            semantic.diff_add_fg,
            semantic.diff_add_bg,
            semantic.diff_add_strong,
        ),
        LineOrigin::Deletion => (
            semantic.diff_del_fg,
            semantic.diff_del_bg,
            semantic.diff_del_strong,
        ),
        LineOrigin::Context => (semantic.overlay0, semantic.base, semantic.base),
    };
    let marker = if selected {
        "▸"
    } else if line.origin == LineOrigin::Context {
        " "
    } else {
        "▎"
    };
    let number = |line: Option<rootle_diff::LineNumber>| {
        line.map(|number| number.to_string()).unwrap_or_default()
    };
    let gutter = format!(
        "{:>digits$} {:>digits$} ",
        number(line.old_line),
        number(line.new_line)
    );
    let remaining = width.saturating_sub(1 + gutter.width());
    let mut spans = vec![
        Span::styled(marker, Style::default().fg(foreground)),
        Span::styled(gutter, Style::default().fg(semantic.overlay0)),
    ];
    let overlay = line
        .changed
        .as_ref()
        .map(|changed| (changed.range(), Style::default().bg(emphasis)));
    let overlays = overlay.as_slice();
    let content = paint_line(
        &line.syntax,
        TextColumns {
            offset: horizontal,
            width: remaining,
        },
        overlays,
    );
    spans.extend(content.spans);
    Line::from(spans).style(Style::default().bg(background))
}

fn count(count: Option<u64>) -> String {
    count
        .map(|count| count.to_string())
        .unwrap_or_else(|| "?".into())
}
