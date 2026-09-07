//! Fields for GlobalSearch.

use super::{
    Block, Borders, Frame, GlobalSearch, Line, Paragraph, Rect, Span, Style, Theme,
    UnicodeWidthStr, fit, fit_spans,
};
use crate::components::global_search::{Focus, grammar};

impl GlobalSearch {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        title: &str,
        value: &str,
        focused: bool,
        input: Option<&crate::components::vim_input::VimInput>,
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
        let prompt = input.map_or_else(
            || Span::styled("❯ ", Style::default().fg(sem.overlay0)),
            |input| input.prompt(focused, theme),
        );
        let mut spans = vec![prompt];
        if styled {
            // Grammar eye candy (plans/0012 M1): qualifiers/quoted
            // literals/negation markers take syntax colors — the spans
            // partition the value byte-exactly, so a query we can't
            // segment renders verbatim and nothing bleeds elsewhere.
            let styled_spans = fit_spans(grammar::style_query(value, theme), width);
            spans.extend(styled_spans);
        } else {
            spans.push(Span::styled(
                fit(value, width),
                Style::default().fg(sem.text),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
        if focused && let Some(input) = input {
            let x = inner.x + 2 + input.cursor() as u16;
            if x < inner.x + inner.width {
                crate::diagnostics::place_cursor(frame, (x, inner.y));
            }
        }
    }

    /// The facet chip row (plans/0012 M3): repos, a dim divider, then
    /// languages — each chip `name·count`, counts live over the whole
    /// accumulated set. The committed chip glows (search-match) as the
    /// visible source of the narrowing; the keyboard cursor takes
    /// selection colors. Whole chips drop off the tail (`…` marks the
    /// cut), or off the head if that's what keeps the cursor visible.
    pub(super) fn render_facets(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if area.height == 0 {
            return;
        }
        let sem = &theme.semantic;
        let chips = self.facets();
        if chips.is_empty() {
            return;
        }
        let focused = self.focus == Focus::Facets;
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
}
