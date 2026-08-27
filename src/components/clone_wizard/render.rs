//! Drawing for the clone wizard: the popup shell (step pips, hint
//! row), the per-screen body — repo checklist, destination browser,
//! summary — and the button row.

use super::{CloneWizard, Focus, Screen};
use crate::components::{centered, scrollbar};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

impl CloneWizard {
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let popup = centered(area, 70, 60);
        frame.render_widget(Clear, popup);

        let (back, next) = self.buttons();
        let (title, subtitle) = match self.screen {
            Screen::Repos => (
                " clone — 1/3 repos ",
                " pick what to clone — ␣ toggles, / filters ",
            ),
            Screen::Destination => (
                " clone — 2/3 destination ",
                " browse to where they land — l descends, h climbs ",
            ),
            Screen::Summary => (
                " clone — 3/3 summary ",
                " review the plan — enter on clone! runs git ",
            ),
        };
        // Step pips in the title row: ● done (green), ● current
        // (accent, bold), ○ ahead (dim).
        let step = self.screen as usize;
        let pip = |i: usize| {
            let (sym, mut style) = if i < step {
                ("●", Style::default().fg(sem.mode_browse))
            } else if i == step {
                (
                    "●",
                    Style::default()
                        .fg(sem.border_focused)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("○", Style::default().fg(sem.surface2))
            };
            style = style.bg(sem.mantle);
            Span::styled(format!(" {sym} "), style)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                title,
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_top(Line::from(vec![pip(0), pip(1), pip(2)]).right_aligned())
            .title_bottom(Span::styled(
                " tab buttons · enter activate · / filter · esc cancel ",
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(inner);
        frame.render_widget(
            Paragraph::new(Span::styled(subtitle, Style::default().fg(sem.hint))),
            rows[0],
        );

        let (lines, cursor_pos) = match self.screen {
            Screen::Repos => (self.repos_lines(theme), Some(self.cursor)),
            Screen::Destination => (self.destination_lines(theme), Some(self.dest_cursor)),
            Screen::Summary => (self.summary_lines(theme), None),
        };
        let total = lines.len();
        let height = rows[1].height as usize;
        // Keep the list cursor visible; the summary scrolls freely.
        if let Some(pos) = cursor_pos {
            if pos < self.scroll {
                self.scroll = pos;
            } else if pos >= self.scroll + height {
                self.scroll = pos + 1 - height.min(pos + 1);
            }
        }
        self.scroll = self.scroll.min(total.saturating_sub(height));
        let scroll = self.scroll;
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), rows[1]);
        scrollbar(frame, popup, height, total, scroll, theme);
        self.render_buttons(frame, rows[2], theme, back, next);
    }

    fn repos_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let sem = &theme.semantic;
        let lines: Vec<Line> = self
            .visible_repos()
            .into_iter()
            .enumerate()
            .map(|(i, orig)| {
                let (repo, on) = &self.repos[orig];
                let on = *on;
                let selected = i == self.cursor && self.focus == Focus::List;
                // Same dot language as VISUAL mode (plans/0004 §1).
                let (mark, mark_color) = if on {
                    ("●", sem.mode_browse)
                } else {
                    ("○", sem.subtext0)
                };
                // v1.4 (plans/0014 #1): archived repos grey out — still
                // cloneable (read-only), visibly not live.
                let style = if selected {
                    Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
                } else if repo.archived {
                    Style::default().fg(sem.subtext0)
                } else {
                    Style::default().fg(sem.text)
                };
                let mut spans = vec![
                    Span::styled(
                        if selected { "▌ " } else { "  " },
                        Style::default().fg(sem.border_focused),
                    ),
                    Span::styled(format!("{mark} "), Style::default().fg(mark_color)),
                    Span::styled(repo.name.clone(), style),
                ];
                let mut note = String::new();
                if repo.archived {
                    note.push_str("archived");
                }
                if let Some(pushed) = &repo.pushed_at {
                    // ISO-8601 → the date is the informative part.
                    let date = pushed.split('T').next().unwrap_or(pushed);
                    if !note.is_empty() {
                        note.push_str(" · ");
                    }
                    note.push_str(date);
                }
                if !note.is_empty() {
                    spans.push(Span::styled(
                        format!("  {note}"),
                        Style::default().fg(sem.subtext0),
                    ));
                }
                Line::from(spans)
            })
            .collect();
        lines
    }

    fn destination_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let sem = &theme.semantic;
        let mut lines = vec![Line::from(vec![
            Span::styled(" dest: ", Style::default().fg(sem.subtext0)),
            Span::styled(
                self.dest.to_string_lossy().into_owned(),
                Style::default()
                    .fg(sem.directory)
                    .add_modifier(Modifier::BOLD),
            ),
        ])];
        for (i, orig) in self.visible_dest().into_iter().enumerate() {
            let entry = &self.dest_entries[orig];
            let selected = i == self.dest_cursor && self.focus == Focus::List;
            let style = if selected {
                Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
            } else {
                Style::default().fg(sem.directory)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "▌ " } else { "  " },
                    Style::default().fg(sem.border_focused),
                ),
                Span::styled(format!("▸ {entry}"), style),
            ]));
        }
        lines
    }

    fn summary_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let sem = &theme.semantic;
        let repos: Vec<&str> = self.checked().map(|r| r.name.as_str()).collect();
        let cmd = Style::default().fg(sem.text);
        let dim = Style::default().fg(sem.subtext0);
        let accent = Style::default().fg(sem.border_focused);

        let mut lines = vec![
            Line::from(Span::styled(
                format!(
                    "clone {} repo{} into {}",
                    repos.len(),
                    if repos.len() == 1 { "" } else { "s" },
                    self.dest.to_string_lossy()
                ),
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(Span::styled("destination", accent)),
        ];
        // The tree the destination will grow: dest/└ org/├ repo…
        let mut by_org: Vec<(String, Vec<String>)> = Vec::new();
        for repo in &repos {
            let (org, name) = repo.split_once('/').unwrap_or(("", repo));
            match by_org.iter_mut().find(|(o, _)| o == org) {
                Some((_, v)) => v.push(name.to_string()),
                None => by_org.push((org.to_string(), vec![name.to_string()])),
            }
        }
        for (org, names) in &by_org {
            lines.push(Line::from(Span::styled(
                format!("  └ {org}/"),
                Style::default()
                    .fg(sem.directory)
                    .add_modifier(Modifier::BOLD),
            )));
            for (i, name) in names.iter().enumerate() {
                let branch = if i + 1 == names.len() { "└" } else { "├" };
                lines.push(Line::from(Span::styled(
                    format!("      {branch} {name}/"),
                    dim,
                )));
            }
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("commands", accent)));
        for repo in &repos {
            lines.push(Line::from(vec![
                Span::styled("  $ ", dim),
                Span::styled(format!("git clone {repo}"), cmd),
                Span::styled(" → ", dim),
                Span::styled(self.target(repo).to_string_lossy().into_owned(), cmd),
            ]));
        }
        lines
    }

    fn render_buttons(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        back: &'static str,
        next: &'static str,
    ) {
        let sem = &theme.semantic;
        let button = |label: &str, idx: usize| {
            let active = self.focus == Focus::Buttons && self.button == idx;
            if active {
                Span::styled(
                    format!(" {label} "),
                    Style::default()
                        .fg(sem.crust)
                        .bg(sem.border_focused)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!(" {label} "),
                    Style::default().fg(sem.subtext0).bg(sem.surface0),
                )
            }
        };
        let line = Line::from(vec![button(back, 0), Span::raw("  "), button(next, 1)]);
        frame.render_widget(
            Paragraph::new(line).alignment(ratatui::layout::Alignment::Center),
            area,
        );
    }
}
