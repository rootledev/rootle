//! Commit composition over shared preview chrome and list/row viewports.

use super::{CommitFocus, CommitLoad, CommitView, rows};
use crate::components::list_view::{RowLayout, RowSpan};
use crate::components::preview::Preview;
use crate::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
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
        let selected = view.selection.selected().get();
        view.viewport.render_virtual(
            frame,
            columns[0],
            file_inner,
            RowLayout {
                total: visible.len(),
                selected: Some(RowSpan::single(selected)),
            },
            |row| {
                rows::file_row(
                    &content.files[visible[row].get()],
                    row == selected,
                    usize::from(file_inner.width),
                    semantic,
                )
            },
            theme,
        );
    }

    if let Some(delta) = &mut view.delta {
        let file = &content.files[delta.file.get()];
        let prepared = file.prepared.as_ref().expect("opening prepares the patch");
        let focused = view.focus == CommitFocus::Preview;
        let block = Preview::pane_block(
            format!(
                " {} · {}/{} ",
                file.label,
                delta.file.get() + 1,
                content.files.len()
            ),
            focused,
            theme,
        );
        let inner = block.inner(columns[1]);
        frame.render_widget(block, columns[1]);
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
                rows::patch_row(
                    &prepared.rows[row],
                    focused && row == selected,
                    prepared.number_width,
                    delta.horizontal,
                    usize::from(inner.width),
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
