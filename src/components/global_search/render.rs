//! Drawing for the search view: the field row, the folded result
//! blocks, and the scope radio popup.

use super::GlobalSearch;
use super::model::{Scope, SearchHit};
use crate::components::pane::fit;
use crate::components::{centered, scrollbar};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

impl GlobalSearch {
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;

        let hint = if self.filtering {
            " type to filter · enter commit · esc cancel "
        } else if self.scope_popup {
            " j/k move · enter done · esc revert "
        } else {
            " tab fields · enter search/open · / filter · esc close "
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
            .constraints([Constraint::Length(3), Constraint::Min(1)])
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
            Some(self.query.cursor()),
        );
        self.render_field(
            frame,
            fields[1],
            theme,
            " scope ",
            &format!("{} ▾", self.scope_label()),
            self.focus == super::Focus::Scope,
            None,
        );
        self.render_field(
            frame,
            fields[2],
            theme,
            " extension ",
            &self.extension.value(),
            self.focus == super::Focus::Extension,
            Some(self.extension.cursor()),
        );

        self.render_results(frame, rows[1], theme);

        if self.scope_popup {
            self.render_scope_popup(frame, area, theme);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        title: &str,
        value: &str,
        focused: bool,
        cursor: Option<usize>,
    ) {
        let sem = &theme.semantic;
        let border = if focused {
            sem.border_focused
        } else {
            sem.border_unfocused
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(border))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(title, Style::default().fg(sem.subtext0)));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let width = inner.width as usize;
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                fit(value, width),
                Style::default().fg(sem.text),
            ))),
            inner,
        );
        if focused && let Some(cursor) = cursor {
            let x = inner.x + cursor as u16;
            if x < inner.x + inner.width {
                frame.set_cursor_position((x, inner.y));
            }
        }
    }

    fn render_results(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let focused = self.focus == super::Focus::Results;
        let border = if focused {
            sem.border_focused
        } else {
            sem.border_unfocused
        };

        let mut title = if self.pending {
            " results — searching… ".to_string()
        } else if let Some(error) = &self.error {
            format!(" results — error: {error} ")
        } else if self.hits.is_empty() && self.submitted_once {
            " results — no matches ".into()
        } else if self.submitted_once {
            let suffix = if self.clipped { " · clipped" } else { "" };
            format!(" results — {}{suffix} ", self.visible().len())
        } else {
            " results ".into()
        };
        if !self.filter_value.is_empty() {
            title = format!("{} /{}", title.trim_end(), self.filter_value);
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(border))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(title, Style::default().fg(sem.subtext0)));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let width = inner.width as usize;
        let height = inner.height as usize;
        let visible = self.visible();

        // Build one block of lines per hit; remember each hit's line
        // range so the selection can be kept in view.
        let mut lines: Vec<Line> = Vec::new();
        let mut ranges: Vec<(usize, usize)> = Vec::new(); // [start, end)
        for (idx, hit) in visible.iter().enumerate() {
            let start = lines.len();
            let selected = idx == self.selected && focused;
            lines.push(self.path_line(hit, width, selected, theme));
            // Disjoint match regions get a dim ellipsis separator.
            let mut prev_no: Option<u32> = None;
            for (no, line) in &hit.preview {
                if let Some(prev) = prev_no
                    && *no > prev + 1
                {
                    lines.push(Line::from(Span::styled(
                        "       ⋮",
                        Style::default().fg(sem.subtext0),
                    )));
                }
                prev_no = Some(*no);
                lines.push(preview_line(*no, line, theme));
            }
            lines.push(Line::raw(""));
            ranges.push((start, lines.len() - 1));
        }
        let total = lines.len();

        // Keep the selected hit visible; J/K free scroll otherwise.
        if focused && let Some((start, end)) = ranges.get(self.selected).copied() {
            if start < self.scroll as usize {
                self.scroll = start as u16;
            } else if end >= self.scroll as usize + height {
                self.scroll = (end + 1).saturating_sub(height) as u16;
            }
        }
        let max_scroll = total.saturating_sub(height) as u16;
        self.scroll = self.scroll.min(max_scroll);

        frame.render_widget(Paragraph::new(lines).scroll((self.scroll, 0)), inner);
        scrollbar(frame, area, height, total, self.scroll as usize, theme);
    }

    fn path_line(
        &self,
        hit: &SearchHit,
        width: usize,
        selected: bool,
        theme: &Theme,
    ) -> Line<'static> {
        let sem = &theme.semantic;
        let gutter = if selected { "▌ " } else { "  " };
        // Grep hits carry a match-count badge (folded multi-matches);
        // file-find hits show the first line number instead. A stale
        // hit (v1.1 located:false) says so until client-side locating
        // self-heals it.
        let meta = if hit.unlocatable {
            "unlocatable".to_string()
        } else if hit.stale {
            "stale".to_string()
        } else if hit.match_count > 0 {
            format!(
                "{} match{}",
                hit.match_count,
                if hit.match_count == 1 { "" } else { "es" }
            )
        } else {
            format!(":{}", hit.line)
        };
        // Cross-repo results need the repo in the row; repo-scope
        // results keep it too — unambiguous everywhere.
        let full = format!("{}/{}", hit.repo, hit.path);
        let path_width = width.saturating_sub(2 + meta.width());
        let path = fit(&full, path_width);
        let pad = width.saturating_sub(2 + path.width() + meta.width());
        let (fg, bg) = if selected {
            (sem.selection_fg, Some(sem.selection_bg))
        } else {
            (sem.text, None)
        };
        let style = {
            let mut s = Style::default().fg(fg).add_modifier(Modifier::BOLD);
            if let Some(bg) = bg {
                s = s.bg(bg);
            }
            s
        };
        let meta_style = {
            let mut s = Style::default().fg(if hit.stale { sem.warning } else { sem.subtext0 });
            if let Some(bg) = bg {
                s = s.bg(bg);
            }
            s
        };
        Line::from(vec![
            Span::styled(gutter, Style::default().fg(sem.border_focused)),
            Span::styled(path, style),
            Span::styled(" ".repeat(pad), meta_style),
            Span::styled(meta, meta_style),
        ])
    }

    fn render_scope_popup(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let items = self.scope_items();
        let height = items.len() as u16 + 2; // rows + border
        let popup_area = centered(area, 40, 30);
        let popup = Rect {
            height: height.min(popup_area.height),
            ..popup_area
        };

        frame.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                " scope ",
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                " j/k move · enter done · esc revert ",
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let mut lines = Vec::new();
        for (idx, (scope, enabled)) in items.iter().enumerate() {
            let radio = if *scope == self.scope { "(•)" } else { "( )" };
            let label = match scope {
                Scope::Repo => match &self.repo {
                    Some(repo) => format!("current repo  repo:{repo}"),
                    None => "current repo  (no repo open)".to_string(),
                },
                Scope::Org => match &self.org {
                    Some(org) => format!("current org  org:{org}"),
                    None => "current org  (no org selected)".to_string(),
                },
                Scope::Global => "all of github".to_string(),
            };
            let cursor = idx == self.scope_cursor;
            let fg = if !enabled {
                sem.subtext0
            } else if cursor {
                sem.selection_fg
            } else {
                sem.text
            };
            let mut style = Style::default().fg(fg);
            if cursor {
                style = style.bg(sem.selection_bg);
            }
            lines.push(Line::from(Span::styled(
                format!("{} {}", radio, label),
                style,
            )));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Preview line: right-aligned line-number gutter + the highlighted
/// code spans (clipped at the area width, like the browser preview).
fn preview_line(no: u32, line: &Line<'static>, theme: &Theme) -> Line<'static> {
    let sem = &theme.semantic;
    let gutter = format!("  {:>4}  ", no);
    let mut spans = vec![Span::styled(gutter, Style::default().fg(sem.subtext0))];
    spans.extend(line.spans.iter().cloned());
    Line::from(spans)
}
