//! Drawing for the search view: the field row, the folded result
//! blocks, and the scope radio popup.

use super::GlobalSearch;
use super::model::{Scope, SearchHit};
use crate::components::pane::fit;
use crate::components::{centered, scrollbar};
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
            Some(self.query.cursor()),
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
            Some(self.extension.cursor()),
            false,
        );

        self.render_facets(frame, rows[1], theme);
        self.render_results(frame, rows[2], theme);

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
        styled: bool,
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
        let width = inner.width.saturating_sub(2) as usize;
        let prompt = if focused {
            Span::styled("❯ ", Style::default().fg(sem.border_focused))
        } else {
            Span::styled("❯ ", Style::default().fg(sem.overlay0))
        };
        let mut spans = vec![prompt];
        if styled {
            // Grammar eye candy (plans/0012 M1): qualifiers/quoted
            // literals/negation markers take syntax colors — the spans
            // partition the value byte-exactly, so a query we can't
            // segment renders verbatim and nothing bleeds elsewhere.
            let styled_spans = fit_spans(super::grammar::style_query(value, theme), width);
            spans.extend(styled_spans);
        } else {
            spans.push(Span::styled(
                fit(value, width),
                Style::default().fg(sem.text),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
        if focused && let Some(cursor) = cursor {
            let x = inner.x + 2 + cursor as u16;
            if x < inner.x + inner.width {
                frame.set_cursor_position((x, inner.y));
            }
        }
    }

    /// The facet chip row (plans/0012 M3): repos, a dim divider, then
    /// languages — each chip `name·count`, counts live over the whole
    /// accumulated set. The committed chip glows (search-match) as the
    /// visible source of the narrowing; the keyboard cursor takes
    /// selection colors. Whole chips drop off the tail (`…` marks the
    /// cut), or off the head if that's what keeps the cursor visible.
    fn render_facets(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if area.height == 0 {
            return;
        }
        let sem = &theme.semantic;
        let chips = self.facets();
        if chips.is_empty() {
            return;
        }
        let focused = self.focus == super::Focus::Facets;
        let width = area.width as usize;

        // Display width of chip idx (text + two-space separator),
        // plus the group divider between the repo and language
        // halves.
        let chip_w = |idx: usize| -> usize {
            format!("{}·{}", chips[idx].id.name, chips[idx].count).width() + 2
        };
        let divider =
            |idx: usize| -> bool { idx > 0 && chips[idx].id.kind != chips[idx - 1].id.kind };
        let w = |idx: usize| chip_w(idx) + usize::from(divider(idx));

        // Chips render from the head and drop off the tail (`…` marks
        // the cut); when the keyboard cursor's chip would fall off,
        // the window starts later instead — the cursor stays visible.
        let mut start = 0;
        if focused {
            let tail_end = {
                let mut used = 8; // " facets "
                let mut idx = 0;
                while idx < chips.len() && used + w(idx) < width {
                    // +1: … reserve
                    used += w(idx);
                    idx += 1;
                }
                idx
            };
            if self.facet_cursor >= tail_end {
                let mut lo = self.facet_cursor;
                let mut used = 9; // label + a leading …
                while lo > 0 && used + w(lo - 1) <= width {
                    used += w(lo - 1);
                    lo -= 1;
                }
                start = lo;
            }
        }

        let mut spans: Vec<Span<'static>> =
            vec![Span::styled(" facets ", Style::default().fg(sem.hint))];
        let mut used = 8;
        if start > 0 {
            spans.push(Span::styled("…", Style::default().fg(sem.overlay0)));
            used += 1;
        }
        for (idx, chip) in chips.iter().enumerate().skip(start) {
            if divider(idx) {
                spans.push(Span::styled("│ ", Style::default().fg(sem.overlay0)));
                used += 2;
            }
            let cursor = focused && idx == self.facet_cursor;
            let active = self.facet.as_ref() == Some(&chip.id);
            let style = if cursor {
                Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
            } else if active {
                Style::default().fg(sem.crust).bg(sem.search_match)
            } else {
                Style::default().fg(sem.subtext0)
            };
            let text = format!("{}·{}  ", chip.id.name, chip.count);
            used += text.width();
            if used + 1 > width {
                spans.push(Span::styled("…", Style::default().fg(sem.overlay0)));
                break;
            }
            spans.push(Span::styled(text, style));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_results(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Expanded (plans/0012 M2): the results area becomes the hit's
        // whole file — the re-used Preview renders into the same rect,
        // no popup, nothing else drawn.
        if let Some(exp) = &mut self.expanded {
            exp.preview.render(frame, area, theme);
            return;
        }
        let sem = &theme.semantic;
        let focused = self.focus == super::Focus::Results;
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
    fn hit_box(
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
