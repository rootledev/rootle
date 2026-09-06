//! The commit viewer's rendering (plans/0028 M4): every diff signal
//! comes from typed data (`rootle_diff` hunks + `LineOrigin`), never
//! from sniffing `+`/`-` in text. Anatomy per strop's 0010 (tuicr
//! lineage): gutter `[sign][old][new][content]`, quiet full-row
//! add/del tints, loud intra-line emphasis via the del/add run
//! pairing engine, structural rows on a band. All colors are theme
//! semantic roles — no literal `Color::` anywhere in this file.

use super::{CommitView, DiffSurface};
use crate::provider::{CommitDetail, CommitFile};
use crate::theme::{Semantic, Theme};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use rootle_diff::{DiffLine, FileDiff, Hunk, LineOrigin};
use unicode_width::UnicodeWidthStr;

/// The sha's 7-char display form.
fn short(sha: &str) -> &str {
    &sha[..7.min(sha.len())]
}

/// A path trimmed from the LEFT for tight titles (roots carry the
/// meaning: `…/components/commit/mod.rs`).
fn truncate_path(path: &str, max: usize) -> String {
    let count = path.chars().count();
    if count <= max {
        path.to_string()
    } else {
        format!(
            "…{}",
            path.chars().skip(count + 1 - max).collect::<String>()
        )
    }
}

/// Build the detail surface: bands + message + changed-files rows.
/// Borrow sequencing: everything immutable builds `rows`; only then
/// does the cursor's keep-visible run.
fn detail_rows<'a>(
    view: &'a CommitView,
    detail: &'a CommitDetail,
    vis: &[usize],
    width: usize,
    sem: &Semantic,
) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    let (added, deleted) = detail.line_stats();
    rows.push(band_row(
        format!(
            " {} · {} · {} ",
            short(&detail.sha),
            detail.author,
            detail.date
        ),
        width,
        &[
            (format!("+{added}"), sem.diff_add_fg),
            (format!("-{deleted}"), sem.diff_del_fg),
        ],
        sem,
    ));
    let msg_lines: Vec<&str> = detail.message.lines().collect();
    let msg_cap = 6.min(msg_lines.len());
    for text in msg_lines.iter().take(msg_cap) {
        rows.push(Line::from(Span::styled(
            format!(" {text}"),
            Style::default().fg(sem.text),
        )));
    }
    if msg_lines.len() > msg_cap {
        rows.push(Line::from(Span::styled(
            " …",
            Style::default().fg(sem.subtext0),
        )));
    }
    rows.push(band_row(
        format!(" files ({}) ", vis.len()),
        width,
        &[],
        sem,
    ));
    let file_row0 = rows.len();
    for (r, &i) in vis.iter().enumerate() {
        rows.push(file_row(&detail.files[i], r == view.list.cursor, sem));
    }
    let _ = file_row0;
    rows
}

fn file_row(f: &CommitFile, is_cursor: bool, sem: &Semantic) -> Line<'static> {
    let mut spans = vec![if is_cursor {
        Span::styled(
            "▌",
            Style::default().fg(sem.selection_fg).bg(sem.selection_bg),
        )
    } else {
        Span::raw(" ")
    }];
    let status_fg = match f.status {
        crate::provider::FileStatus::Added => sem.diff_add_fg,
        crate::provider::FileStatus::Removed => sem.diff_del_fg,
        _ => sem.subtext0,
    };
    spans.push(Span::styled(
        format!("{} ", f.status.glyph()),
        Style::default().fg(status_fg),
    ));
    spans.push(Span::styled(
        f.path.clone(),
        if is_cursor {
            Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
        } else {
            Style::default().fg(sem.text)
        },
    ));
    if let (Some(a), Some(d)) = (f.additions, f.deletions) {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("+{a} "),
            Style::default().fg(sem.diff_add_fg),
        ));
        spans.push(Span::styled(
            format!("-{d} "),
            Style::default().fg(sem.diff_del_fg),
        ));
    }
    Line::from(spans)
}

/// A structural band row: label left, counts right, `diff_band` under
/// everything (a band, not an accent).
fn band_row(
    label: String,
    width: usize,
    right: &[(String, ratatui::style::Color)],
    sem: &Semantic,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        label,
        Style::default().fg(sem.text).bg(sem.diff_band),
    )];
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    for (text, fg) in right {
        let pad = width.saturating_sub(used + text.chars().count()).min(width);
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().bg(sem.diff_band),
        ));
        spans.push(Span::styled(
            format!("{text} "),
            Style::default().fg(*fg).bg(sem.diff_band),
        ));
    }
    Line::from(spans).style(Style::default().bg(sem.diff_band))
}

/// The delta surface's flat rows (strop's DiffRow addressing: stats,
/// then per hunk a header then its lines — cursor and row count walk
/// the same arithmetic).
fn delta_rows(fd: &FileDiff, digits: usize, sem: &Semantic, width: usize) -> Vec<Line<'static>> {
    let mut rows =
        Vec::with_capacity(1 + fd.hunks.iter().map(|h| 1 + h.lines.len()).sum::<usize>());
    let (a, d) = (fd.added, fd.deleted);
    rows.push(band_row(
        String::new(),
        width,
        &[
            (format!("+{a}"), sem.diff_add_fg),
            (format!("-{d}"), sem.diff_del_fg),
        ],
        sem,
    ));
    for hunk in &fd.hunks {
        rows.push(band_row(format!(" {} ", hunk.header()), width, &[], sem));
        for (idx, dl) in hunk.lines.iter().enumerate() {
            rows.push(diff_line_row(dl, hunk, idx, digits, sem));
        }
    }
    rows
}

/// One diff content row: `[sign][old][new][content]` — quiet origin
/// tint under the whole row, the emphasis engine's byte range getting
/// the strong tint, absent line numbers blank (never `0`).
fn diff_line_row(
    dl: &DiffLine,
    hunk: &Hunk,
    idx: usize,
    digits: usize,
    sem: &Semantic,
) -> Line<'static> {
    let (sign, origin_fg, quiet) = match dl.origin {
        LineOrigin::Addition => ("▎", sem.diff_add_fg, sem.diff_add_bg),
        LineOrigin::Deletion => ("▎", sem.diff_del_fg, sem.diff_del_bg),
        LineOrigin::Context => (" ", sem.overlay0, sem.base),
    };
    let num = |n: Option<u32>| match n {
        Some(n) => format!("{n:>width$} ", width = digits),
        None => format!("{:>width$} ", "", width = digits),
    };
    let text = dl.text.trim_end_matches('\r');
    let strong = match dl.origin {
        LineOrigin::Addition => sem.diff_add_strong,
        LineOrigin::Deletion => sem.diff_del_strong,
        LineOrigin::Context => sem.base,
    };
    let mut spans = vec![
        Span::styled(sign, Style::default().fg(origin_fg).bg(quiet)),
        Span::styled(
            format!("{}{} ", num(dl.old_lineno), num(dl.new_lineno)),
            Style::default().fg(sem.overlay0).bg(quiet),
        ),
    ];
    match rootle_diff::emphasis_ranges(hunk, idx) {
        Some((start, end)) if end > start && text.is_char_boundary(start.min(text.len())) => {
            let (start, end) = (start.min(text.len()), end.min(text.len()));
            spans.push(Span::styled(
                text[..start].to_string(),
                Style::default().fg(sem.text).bg(quiet),
            ));
            spans.push(Span::styled(
                text[start..end].to_string(),
                Style::default().fg(sem.text).bg(strong),
            ));
            spans.push(Span::styled(
                text[end..].to_string(),
                Style::default().fg(sem.text).bg(quiet),
            ));
        }
        _ => spans.push(Span::styled(
            text.to_string(),
            Style::default().fg(sem.text).bg(quiet),
        )),
    }
    Line::from(spans).style(Style::default().bg(quiet))
}

fn digits(fd: &FileDiff) -> usize {
    let max = fd
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter_map(|l| l.new_lineno.or(l.old_lineno))
        .max()
        .unwrap_or(0);
    max.to_string().len().max(3)
}

/// Row count for cursor clamping without building rows.
pub(super) fn delta_row_count(diff: &DiffSurface) -> usize {
    match &diff.parsed {
        Some(fd) => 1 + fd.hunks.iter().map(|h| 1 + h.lines.len()).sum::<usize>(),
        None => 1,
    }
}

pub(super) fn draw(view: &mut CommitView, frame: &mut Frame, area: Rect, theme: &Theme) {
    let sem = &theme.semantic;
    let inner = {
        let title = if let (Some(detail), Some(diff)) = (&view.detail, &view.diff)
            && let Some(file) = detail.files.get(diff.file)
        {
            format!(
                " {} · {}/{} ",
                truncate_path(&file.path, area.width.saturating_sub(14) as usize),
                diff.file + 1,
                detail.files.len()
            )
        } else if view.filter_value.is_empty() {
            format!(" commit {} ", short(&view.sha))
        } else {
            format!(" commit {} /{} ", short(&view.sha), view.filter_value)
        };
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    };

    if let Some(error) = &view.failed {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {error}"),
                Style::default().fg(sem.error),
            ))),
            inner,
        );
        return;
    }
    if view.loading {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " loading commit…",
                Style::default().fg(sem.subtext0),
            ))),
            inner,
        );
        return;
    }
    let Some(detail) = &view.detail else {
        return;
    };

    if let Some(diff) = &mut view.diff {
        // Lazy parse: the provider's patch text → typed hunks.
        if diff.parsed.is_none() && !diff.binary {
            match &detail.files[diff.file].patch {
                Some(patch) => diff.parsed = Some(FileDiff::parse(patch)),
                None => diff.binary = true,
            }
        }
        let rows = if diff.binary {
            vec![Line::from(Span::styled(
                format!(" {} — binary file", detail.files[diff.file].path),
                Style::default().fg(sem.subtext0),
            ))]
        } else if let Some(fd) = &diff.parsed {
            delta_rows(fd, digits(fd), sem, inner.width as usize)
        } else {
            vec![Line::from(Span::styled(
                " parsing delta…",
                Style::default().fg(sem.subtext0),
            ))]
        };
        let total = rows.len();
        diff.cursor = diff.cursor.min(total.saturating_sub(1));
        let height = inner.height as usize;
        diff.scroll = diff
            .cursor
            .saturating_sub(height - 1)
            .min(total.saturating_sub(height)) as u16;
        frame.render_widget(Paragraph::new(rows).scroll((diff.scroll, 0)), inner);
        crate::components::scrollbar(frame, area, height, total, diff.scroll as usize, theme);
    } else {
        let vis = view.visible_files(detail);
        let rows = detail_rows(view, detail, &vis, inner.width as usize, sem);
        let total = rows.len();
        view.list.keep_visible(inner.height as usize, total);
        frame.render_widget(Paragraph::new(rows).scroll((view.list.scroll, 0)), inner);
        crate::components::scrollbar(
            frame,
            area,
            inner.height as usize,
            total,
            view.list.scroll as usize,
            theme,
        );
    }
}
