//! Drawing for the search view: the field row, the folded result
//! blocks, and the scope radio popup.

mod fields;
mod results;
mod scope;

use super::GlobalSearch;
use super::model::{Scope, SearchHit};
use crate::components::pane::fit;
use crate::components::{centered_clamped, scrollbar};
use crate::keymap;
use crate::mode::Mode;
use crate::theme::Theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

impl GlobalSearch {
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;

        let hint = if self.filtering {
            " type to filter · enter commit · esc cancel ".into()
        } else if self.scope_popup {
            " j/k move · enter done · esc revert ".into()
        } else if self.finding {
            keymap::hint_row(keymap::hints(Mode::Find))
        } else if self.focus == super::Focus::Error {
            keymap::hint_row(keymap::search_error())
        } else if self.focus == super::Focus::Facets {
            keymap::hint_row(keymap::search_facets())
        } else if self.expanded.is_some() {
            keymap::hint_row(keymap::search_file())
        } else {
            keymap::hint_row(keymap::search_results())
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                self.kind.title(),
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(hint, Style::default().fg(sem.hint)));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                // The chip row (plans/0012 M3) exists only once hits
                // hold a facet; a zero-height row keeps the layout
                // indices stable either way.
                Constraint::Length(if self.facets().is_empty() { 0 } else { 1 }),
                Constraint::Min(1),
            ])
            .split(inner);

        let fields = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[0]);

        self.render_field(
            frame,
            fields[0],
            theme,
            " query ",
            &self.query.value(),
            self.focus == super::Focus::Query,
            Some(&self.query),
            true,
        );
        self.render_field(
            frame,
            fields[1],
            theme,
            " scope ",
            &format!("{} ▾", self.scope_label()),
            self.focus == super::Focus::Scope,
            None,
            false,
        );
        self.render_field(
            frame,
            fields[2],
            theme,
            " extension ",
            &self.extension.value(),
            self.focus == super::Focus::Extension,
            Some(&self.extension),
            false,
        );

        self.render_facets(frame, rows[1], theme);
        self.render_results(frame, rows[2], theme);

        if self.scope_popup {
            self.render_scope_popup(frame, area, theme);
        }
    }
}

/// fit() for styled spans: cut whole spans at the width, then the
/// last span per-char.
fn fit_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0;
    for span in spans {
        let w = span.content.width();
        if used + w <= width {
            used += w;
            out.push(span);
        } else {
            let mut cut = String::new();
            for c in span.content.chars() {
                let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                if used + cw > width {
                    break;
                }
                cut.push(c);
                used += cw;
            }
            out.push(Span::styled(cut, span.style));
            break;
        }
    }
    out
}

/// Preview line: right-aligned line-number gutter with the same dim
/// `│` divider the browser preview uses, then the highlighted spans.
fn preview_line(no: u32, line: &Line<'static>, theme: &Theme) -> Line<'static> {
    let sem = &theme.semantic;
    let mut spans = vec![
        Span::styled(format!("{no:>4} "), Style::default().fg(sem.subtext0)),
        Span::styled("│ ", Style::default().fg(sem.overlay0)),
    ];
    spans.extend(line.spans.iter().cloned());
    Line::from(spans)
}
