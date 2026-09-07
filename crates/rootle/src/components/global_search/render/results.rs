//! Results for GlobalSearch.

use super::{
    Block, BorderType, Borders, Frame, GlobalSearch, Line, Modifier, Paragraph, Rect, SearchHit,
    Span, Style, Theme, UnicodeWidthStr, fit, preview_line, scrollbar, symbols,
};
use crate::components::global_search::Focus;

impl GlobalSearch {
    pub(super) fn render_results(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Expanded (plans/0012 M2): the results area becomes the hit's
        // whole file — the re-used Preview renders into the same rect,
        // no popup, nothing else drawn.
        if let Some(exp) = &mut self.expanded {
            exp.preview.render(frame, area, theme);
            return;
        }
        let sem = &theme.semantic;
        let focused = self.focus == Focus::Results;
        let border = if focused {
            sem.border_focused
        } else {
            sem.border_unfocused
        };
        let mut title = if let Some(error) = &self.error {
            format!(" results — error: {error} ")
        } else if self.pending && self.hits.is_empty() {
            " results — searching… ".to_string()
        } else if self.pending {
            // v1.3: hits stream in — the count climbs live.
            let suffix = if self.clipped { " · clipped" } else { "" };
            format!(" results — {} · streaming{suffix} ", self.visible().len())
        } else if self.hits.is_empty() && self.submitted_once {
            " results — no matches ".into()
        } else if self.submitted_once {
            let mut suffix = String::new();
            if let Some(as_of) = &self.index_as_of {
                // Indexed backends say when the index was built — a
                // lagging index is worth the badge.
                let short: String = as_of.chars().take(19).collect();
                suffix.push_str(&format!(" · index {short}"));
            }
            if self.clipped {
                suffix.push_str(" · clipped");
            }
            if let Some(note) = &self.search_ref_note {
                suffix.push_str(&format!(" · {note}"));
            }
            // plans/0012 M1 honesty chips: hits rootle subtracted
            // client-side (the backend couldn't express the grammar),
            // and tokens nobody could express.
            if self.client_filtered > 0 {
                suffix.push_str(&format!(" · filtered {}", self.client_filtered));
            }
            if !self.unfiltered.is_empty() {
                suffix.push_str(&format!(" · unfiltered: {}", self.unfiltered.join(" ")));
            }
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

        // Build one bordered box per hit; remember each hit's line
        // range so the selection can be kept in view.
        let mut lines: Vec<Line> = Vec::new();
        let mut ranges: Vec<(usize, usize)> = Vec::new(); // [start, end)
        for (idx, hit) in visible.iter().enumerate() {
            let start = lines.len();
            let selected = idx == self.selected && focused;
            lines.extend(self.hit_box(hit, width, selected, theme));
            ranges.push((start, lines.len()));
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

    /// One hit as a bordered box — the bat/delta file-header
    /// convention in the pane idiom this TUI already uses: the
    /// filename rides the top rule as the box's title (the same
    /// decoration the pane titles carry), the match badge closes the
    /// rule on the right, and the match lines keep their `│` gutter
    /// between the box's rails. Selection paints rails + title instead
    /// of a `▌` gutter row. The border shape follows `[ui] border`.
    pub(super) fn hit_box(
        &self,
        hit: &SearchHit,
        width: usize,
        selected: bool,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        let sem = &theme.semantic;
        let set = match theme.border_type() {
            BorderType::Rounded => symbols::border::ROUNDED,
            BorderType::Thick => symbols::border::THICK,
            BorderType::Double => symbols::border::DOUBLE,
            BorderType::Plain => symbols::border::PLAIN,
            // [ui] border only offers the four above.
            _ => symbols::border::PLAIN,
        };
        let rail = Style::default().fg(if selected {
            sem.border_focused
        } else {
            sem.overlay0
        });
        let h = set.horizontal_top;

        let meta = if hit.unlocatable {
            "unlocatable".to_string()
        } else if hit.stale {
            "stale".to_string()
        } else if hit.match_count > 0 {
            // Grep hits carry a match-count badge (folded
            // multi-matches); file-find hits show the anchor line.
            format!(
                "{} match{}",
                hit.match_count,
                if hit.match_count == 1 { "" } else { "es" }
            )
        } else {
            format!(":{}", hit.line)
        };
        let meta_style = Style::default().fg(if hit.stale { sem.warning } else { sem.subtext0 });
        let title_style = {
            let mut s = Style::default()
                .fg(if selected { sem.selection_fg } else { sem.text })
                .add_modifier(Modifier::BOLD);
            if selected {
                s = s.bg(sem.selection_bg);
            }
            s
        };

        // Cross-repo results need the repo in the title; repo-scope
        // results keep it too — unambiguous everywhere.
        let full = format!("{}/{}", hit.repo, hit.path);
        let inner = width.saturating_sub(2); // between the corners
        // Top rule: ╭─ path ─fill─ meta ─╮
        let fixed = 2 + meta.width() + 6; // "─ " … " " + meta + " ─"
        let path = fit(&full, inner.saturating_sub(fixed).max(8));
        let fill = inner.saturating_sub(2 + path.width() + 1 + 1 + meta.width() + 2);
        let mut lines = vec![Line::from(vec![
            Span::styled(set.top_left.to_string(), rail),
            Span::styled(h.to_string(), rail),
            Span::raw(" "),
            Span::styled(path, title_style),
            Span::raw(" "),
            Span::styled(h.repeat(fill), rail),
            Span::raw(" "),
            Span::styled(meta, meta_style),
            Span::raw(" "),
            Span::styled(h.to_string(), rail),
            Span::styled(set.top_right.to_string(), rail),
        ])];

        // Content lines between the rails; disjoint match regions get
        // a dim ellipsis aligned over the gutter divider.
        let pad = |line: Line<'static>| -> Line<'static> {
            let w = line.width();
            let mut spans = vec![Span::styled(set.vertical_left.to_string(), rail)];
            spans.extend(line.spans);
            spans.push(Span::raw(" ".repeat(inner.saturating_sub(w))));
            spans.push(Span::styled(set.vertical_right.to_string(), rail));
            Line::from(spans)
        };
        let mut prev_no: Option<u32> = None;
        for (no, line) in &hit.preview {
            if let Some(prev) = prev_no
                && *no > prev + 1
            {
                lines.push(pad(Line::from(Span::styled(
                    format!("{:>6} ", "⋮"),
                    Style::default().fg(sem.subtext0),
                ))));
            }
            prev_no = Some(*no);
            lines.push(pad(preview_line(*no, line, theme)));
        }

        lines.push(Line::from(vec![
            Span::styled(set.bottom_left.to_string(), rail),
            Span::styled(h.repeat(inner), rail),
            Span::styled(set.bottom_right.to_string(), rail),
        ]));
        lines
    }
}
