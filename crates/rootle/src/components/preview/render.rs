//! Render for Preview.

use super::{
    Block, Borders, Constraint, Direction, EntryKind, Frame, Layout, Line, Modifier, Paragraph,
    Preview, PreviewContent, Rect, Span, Style, Theme, UnicodeWidthStr, Wrap, chip_line,
    fit_middle, lens,
};

impl Preview {
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        // While finding, the title carries the query: `main.rs /ratatui`.
        let title = match &self.find {
            Some(f) => format!(" {} /{} ", self.title, f.query),
            None => format!(" {} ", self.title),
        };
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(if self.focused {
                sem.border_focused
            } else {
                sem.border_unfocused
            }))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(title, Style::default().fg(sem.subtext0)));
        if let Some(readout) = self.readout() {
            block = block.title_bottom(
                Line::from(Span::styled(
                    format!(" {readout} "),
                    Style::default().fg(sem.subtext0),
                ))
                .right_aligned(),
            );
        }
        // Footer: language + line count for real file content.
        if self.numbered {
            let meta = match &self.lang {
                Some(lang) => format!(" {lang} · {} lines ", self.line_count),
                None => format!(" {} lines ", self.line_count),
            };
            block = block.title_bottom(
                Line::from(Span::styled(meta, Style::default().fg(sem.overlay0))).left_aligned(),
            );
        }

        let cursored = self.line_count > 0;
        let cursor = self.cursor as usize;
        let mut lines: Vec<Line> = match &self.content {
            PreviewContent::Empty => {
                vec![Line::from(Span::styled(
                    "nothing selected",
                    Style::default().fg(sem.overlay0),
                ))]
            }
            PreviewContent::Highlighted(lines) => lines.clone(),
            PreviewContent::Text(text) => text
                .lines()
                .map(|l| {
                    Line::from(Span::styled(
                        l.replace('\t', "    "),
                        Style::default().fg(sem.text),
                    ))
                })
                .collect(),
            PreviewContent::DirSummary(children) => children
                .iter()
                .map(|e| match e.kind {
                    EntryKind::File => {
                        Line::from(Span::styled(e.name.clone(), Style::default().fg(sem.file)))
                    }
                    _ => Line::from(Span::styled(
                        format!("{}/", e.name),
                        Style::default()
                            .fg(sem.directory)
                            .add_modifier(Modifier::BOLD),
                    )),
                })
                .collect(),
            PreviewContent::Binary { size } => vec![Line::from(Span::styled(
                format!("binary file · {} bytes", size),
                Style::default().fg(sem.warning),
            ))],
        };
        // Find chips: split spans at match boundaries; matches ride the
        // search_match chip (grep-view parity), the current match the
        // warning accent so n/N reads at a glance.
        if self.numbered
            && let Some(find) = &self.find
            && !find.matches.is_empty()
        {
            let match_style = Style::default()
                .fg(sem.crust)
                .bg(sem.search_match)
                .add_modifier(Modifier::BOLD);
            let current_style = Style::default()
                .fg(sem.crust)
                .bg(sem.warning)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            for (i, line) in lines.iter_mut().enumerate() {
                let ranges: Vec<(usize, usize, bool)> = find
                    .matches
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.line as usize == i)
                    .map(|(idx, m)| (m.start, m.end, idx == find.current))
                    .collect();
                if !ranges.is_empty() {
                    *line = chip_line(line, &ranges, match_style, current_style);
                }
            }
        }
        // Selection tint on the cursor line (text content only).
        if cursored && let Some(line) = lines.get_mut(cursor) {
            line.style = Style::default().bg(sem.selection_bg);
        }
        // Visual-lines (vim V): the range tints like the cursor line.
        if let Some((lo, hi)) = self.visual_range() {
            for i in (lo as usize - 1)..=(hi as usize - 1).min(lines.len().saturating_sub(1)) {
                if let Some(line) = lines.get_mut(i) {
                    line.style = Style::default().bg(sem.selection_bg);
                }
            }
        }
        // Line-number gutter, bat/helix style: sign column (▶ marks
        // the cursor line, tuicr-style) + space + right-aligned dim
        // numbers + a dim `│` divider before the content. The cursor
        // line's number reads bold (vim CursorLineNr).
        if self.numbered {
            let width = self.line_count.max(1).to_string().len();
            for (i, line) in lines.iter_mut().enumerate() {
                let cursor_line = i == cursor;
                let num_style = if cursor_line {
                    Style::default().fg(sem.text).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(sem.overlay0)
                };
                line.spans
                    .insert(0, Span::styled(format!("{:>width$}", i + 1), num_style));
                line.spans.insert(
                    0,
                    Span::styled(
                        if cursor_line { "▶ " } else { "  " },
                        if cursor_line {
                            Style::default()
                                .fg(sem.border_focused)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                );
                line.spans
                    .insert(2, Span::styled(" │ ", Style::default().fg(sem.overlay0)));
            }
        }

        // Blame lens (plans/0016 M1c): a margin before the gutter —
        // sha + author at each run's first line, a dim dot leader on
        // continuations (fugitive-style runs, not a per-line column).
        if self.numbered
            && let Some(blame) = &self.blame
        {
            for (i, line) in lines.iter_mut().enumerate() {
                let spans: Vec<Span<'static>> = match blame.get(i).cloned().flatten() {
                    Some(m) => vec![
                        Span::styled(m.sha, Style::default().fg(sem.warning)),
                        Span::styled(
                            // 8 cells: real names truncate, short ones pad.
                            format!(" {:<8.8}", m.author),
                            Style::default().fg(sem.subtext0),
                        ),
                        Span::styled(" │ ".to_string(), Style::default().fg(sem.overlay0)),
                    ],
                    None => vec![Span::styled(
                        // 19 cells: sha(7) + ' ' + author(8) + ' │ '.
                        "           ·    ".to_string() + " │ ",
                        Style::default().fg(sem.overlay0),
                    )],
                };
                for (j, sp) in spans.into_iter().enumerate() {
                    line.spans.insert(j, sp);
                }
            }
        }

        // The header band (GitHub's file header, plans/0016 M1b): on
        // file content one row under the border carries the full path
        // — plus the at-commit context on the right when viewing
        // history — on a surface0 strip so it reads as chrome, not
        // content.
        let band = self.numbered && self.band_path.is_some();
        let inner = block.inner(area);
        self.viewport = inner.height.saturating_sub(band as u16);
        self.clamp_scroll(self.viewport);
        frame.render_widget(block, area);
        let content_area = if band {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .split(inner);
            let path = self.band_path.clone().unwrap_or_default();
            let mut spans = vec![Span::styled(
                format!(" {path}"),
                Style::default()
                    .fg(sem.text)
                    .bg(sem.surface0)
                    .add_modifier(Modifier::BOLD),
            )];
            let left_w = 1 + UnicodeWidthStr::width(path.as_str());
            if let Some(ctx) = &self.band_context {
                // sha · subject · author · date — sha in the accent the
                // history lens uses, the rest dim. Under width pressure
                // the tail sheds date, then author; the sha and subject
                // stay. One space of separation from the path, always.
                let room = (inner.width as usize).saturating_sub(left_w + 1);
                let full = format!(
                    "{} · {} · {} · {}",
                    ctx.sha, ctx.subject, ctx.author, ctx.date
                );
                let short = format!("{} · {} · {}", ctx.sha, ctx.subject, ctx.author);
                let shorter = format!("{} · {}", ctx.sha, ctx.subject);
                let fits = |s: &str| UnicodeWidthStr::width(s) <= room;
                let text = if fits(&full) {
                    full
                } else if fits(&short) {
                    short
                } else if fits(&shorter) {
                    shorter
                } else {
                    fit_middle(&ctx.sha, room)
                };
                let pad = room.saturating_sub(UnicodeWidthStr::width(text.as_str()));
                spans.push(Span::styled(
                    " ".repeat(pad + 1),
                    Style::default().bg(sem.surface0),
                ));
                // Segmented: sha hot, the rest dim — the row reads at a
                // glance instead of shouting in one color.
                spans.push(Span::styled(
                    ctx.sha
                        .chars()
                        .take(lens::sha_len(&text, ctx))
                        .collect::<String>(),
                    Style::default().fg(sem.warning).bg(sem.surface0),
                ));
                spans.push(Span::styled(
                    text[lens::sha_len(&text, ctx)..].to_string(),
                    Style::default().fg(sem.subtext0).bg(sem.surface0),
                ));
            }
            let band_line = Line::from(spans);
            let band_w: usize = band_line.spans.iter().map(|s| s.content.width()).sum();
            let mut band_spans = band_line.spans;
            band_spans.push(Span::styled(
                " ".repeat((inner.width as usize).saturating_sub(band_w)),
                Style::default().bg(sem.surface0),
            ));
            frame.render_widget(Paragraph::new(Line::from(band_spans)), rows[0]);
            rows[1]
        } else {
            inner
        };
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((self.scroll, 0))
                .wrap(Wrap { trim: false }),
            content_area,
        );
        // House style: anything that scrolls shows a scrollbar.
        if self.numbered {
            crate::components::scrollbar(
                frame,
                area,
                self.viewport as usize,
                self.line_count as usize,
                self.scroll as usize,
                theme,
            );
        }
    }

    /// Border readout for text content: `3/41`, or `m/n · 3/41`
    /// (match of matches · line of total) while a find is active.
    pub(super) fn readout(&self) -> Option<String> {
        if self.line_count == 0 {
            return None;
        }
        let pos = format!("{}/{}", self.cursor + 1, self.line_count);
        let pos = match self.visual_range() {
            // VISUAL marker + the range — vim's -- VISUAL -- line.
            Some((lo, hi)) => format!("VISUAL {lo}-{hi} · {pos}"),
            None => pos,
        };
        match &self.find {
            Some(f) if !f.query.is_empty() => {
                let cur = if f.matches.is_empty() {
                    0
                } else {
                    f.current + 1
                };
                Some(format!("{cur}/{} · {pos}", f.matches.len()))
            }
            _ => Some(pos),
        }
    }
}
