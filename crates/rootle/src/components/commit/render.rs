//! Commit composition over shared preview chrome and list/row viewports.

use super::{CommitFocus, CommitLoad, CommitView, files::FileTreeRow, rows};
use crate::components::list_view::{RowIndex, RowLayout, RowSpan};
use crate::components::preview::Preview;
use crate::components::text::{TextColumns, paint_line};
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

pub(super) fn draw(view: &mut CommitView, frame: &mut Frame, area: Rect, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    let semantic = &theme.semantic;
    let shell = Preview::pane_block(format!(" commit {} ", view.sha_short()), false, theme);
    let inner = shell.inner(area);
    frame.render_widget(shell, area);
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
    let metadata = Rect::new(inner.x, inner.y, inner.width, 1);
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
        inner.y.saturating_add(1),
        inner.width,
        inner.height.saturating_sub(1),
    );
    if body.is_empty() {
        return;
    }
    // On narrow terminals the same children stack; neither acquires a
    // different interaction model or silently disappears behind an overlay.
    let columns = if body.width >= 60 {
        Layout::horizontal([
            Constraint::Length((body.width / 3).clamp(24, 44)),
            Constraint::Min(0),
        ])
        .split(body)
    } else {
        Layout::vertical([
            Constraint::Length((body.height / 3).min(6)),
            Constraint::Min(0),
        ])
        .split(body)
    };
    let visible = view.visible_files(content);
    let mut title = format!(" files ({}) ", visible.len());
    if !view.filter.is_empty() || view.filter.active() {
        title.push_str(&format!("/{} ", view.filter.text()));
    }
    if content.detail.truncated {
        title.push_str("· truncated by provider ");
    }
    let file_block = Preview::pane_block(title, view.focus == CommitFocus::Files, theme);
    let file_inner = file_block.inner(columns[0]);
    frame.render_widget(file_block, columns[0]);
    if visible.is_empty() {
        frame.render_widget(Paragraph::new(" no matching files"), file_inner);
    } else {
        let selected_file = visible.get(view.selection.selected().get()).copied();
        let tree_rows = content.tree.visible_rows(&visible);
        let selected_row = tree_rows.iter().position(
            |row| matches!(row, FileTreeRow::File { file, .. } if Some(*file) == selected_file),
        );
        view.viewport.render_virtual(
            frame,
            columns[0],
            file_inner,
            RowLayout {
                total: tree_rows.len(),
                selected: selected_row.map(RowSpan::single),
            },
            |row| match tree_rows[row] {
                FileTreeRow::Directory { label, depth } => {
                    rows::directory_row(label, *depth, usize::from(file_inner.width), semantic)
                }
                FileTreeRow::File { file, depth } => rows::file_row(
                    &content.files[file.get()],
                    Some(*file) == selected_file,
                    *depth,
                    usize::from(file_inner.width),
                    semantic,
                ),
            },
            theme,
        );
    }

    if let Some(delta) = &mut view.delta {
        let file = &content.files[delta.file.get()];
        let prepared = file.prepared.as_ref().expect("opening prepares the patch");
        let focused = view.focus == CommitFocus::Preview;
        let mut block = Preview::pane_block(
            format!(
                " {} · {}/{} ",
                file.label,
                view.selection.selected().get() + 1,
                visible.len()
            ),
            focused,
            theme,
        );
        if !delta.search.query().is_empty() {
            block = block.title_bottom(Line::from(format!(
                " /{} · {} ",
                delta.search.query(),
                delta.search.readout()
            )));
        }
        let mut inner = block.inner(columns[1]);
        frame.render_widget(block, columns[1]);
        if delta.search.editing() && inner.height > 0 {
            let prompt = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
            draw_search_prompt(frame, prompt, &delta.search, theme);
            inner.height -= 1;
        }
        delta.text_width = usize::from(inner.width).saturating_sub(2 * prepared.number_width + 3);
        let selected = delta.selection.selected().get();
        delta.viewport.render_virtual(
            frame,
            columns[1],
            inner,
            RowLayout {
                total: prepared.rows.len(),
                selected: Some(RowSpan::single(selected)),
            },
            |row| {
                let overlays = delta.search.overlays(RowIndex::new(row), theme);
                rows::patch_row(
                    &prepared.rows[row],
                    rows::DiffRowLayout {
                        selected: focused && row == selected,
                        number_width: prepared.number_width,
                        horizontal: delta.horizontal,
                        width: usize::from(inner.width),
                    },
                    &overlays,
                    semantic,
                )
            },
            theme,
        );
    } else {
        view.message.focused = view.focus == CommitFocus::Preview;
        view.message.render(frame, columns[1], theme);
    }
}

fn draw_search_prompt(
    frame: &mut Frame,
    area: Rect,
    search: &super::search::DiffSearch,
    theme: &Theme,
) {
    let available = usize::from(area.width.saturating_sub(2));
    let cursor = search.cursor_column();
    let offset = cursor.saturating_sub(available.saturating_sub(1));
    let mut line = Line::from(Span::styled(
        "/ ",
        Style::default().fg(theme.semantic.border_focused),
    ));
    let query = Line::from(Span::styled(
        search.query(),
        Style::default().fg(theme.semantic.text),
    ));
    line.spans.extend(
        paint_line(
            &query,
            TextColumns {
                offset,
                width: available,
            },
            &[],
        )
        .spans,
    );
    frame.render_widget(Paragraph::new(line), area);
    if area.width > 2 && available > 0 {
        crate::diagnostics::place_cursor(frame, (area.x + 2 + (cursor - offset) as u16, area.y));
    }
}
