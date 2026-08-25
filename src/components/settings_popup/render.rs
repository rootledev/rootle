//! Drawing for the settings popup: the section sidebar, the active
//! section's rows (text / bool dots / radio lists), the in-place edit
//! cursor, and the per-section scrollbar.

use super::SettingsPopup;
use super::sections::Row;
use crate::components::pane::fit;
use crate::components::{centered, scrollbar};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

/// Left gutter before the label: the `▌` selection marker (house style).
const GUTTER: usize = 2;
/// Label column width (longest: `read_only`) plus a gap to the value.
const LABEL: usize = 11;
/// Sidebar column: marker + section name + one-word blurb.
const SIDEBAR: u16 = 20;

impl SettingsPopup {
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // After a theme commit the popup renders with the previewed
        // palette, not the app's.
        let theme = self.preview.unwrap_or(*theme);
        let sem = &theme.semantic;
        let popup = centered(area, 72, 62);
        frame.render_widget(Clear, popup);

        let hint = if self.editing.is_some() {
            " enter commit · esc stop editing "
        } else {
            " tab/h/l section · j/k row · ␣/enter change · esc save "
        };
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                " settings ",
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(hint, Style::default().fg(sem.hint)));
        if self.dirty {
            block = block.title_top(
                Line::from(Span::styled(
                    " ● unsaved ",
                    Style::default().fg(sem.warning),
                ))
                .right_aligned(),
            );
        }
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR), Constraint::Min(1)])
            .split(inner);
        self.render_sidebar(frame, cols[0], &theme);
        self.render_section(frame, cols[1], &theme);
    }

    /// Section list: `▸ name` + one-word blurb; the active section
    /// carries the selection background across the full column.
    fn render_sidebar(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let mut lines = vec![Line::raw("")];
        for (i, s) in self.sections.iter().enumerate() {
            let active = i == self.section;
            let bg = if active { sem.selection_bg } else { sem.mantle };
            let marker = if active {
                Span::styled("▸ ", Style::default().fg(sem.border_focused).bg(bg))
            } else {
                Span::styled("  ", Style::default().bg(bg))
            };
            let name = if active {
                Style::default()
                    .fg(sem.selection_fg)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(sem.subtext0).bg(bg)
            };
            let blurb = Style::default()
                .fg(if active { sem.subtext0 } else { sem.overlay0 })
                .bg(bg);
            let used = 2 + 9 + s.blurb.len();
            let pad = (area.width as usize).saturating_sub(used);
            lines.push(Line::from(vec![
                marker,
                Span::styled(format!("{:<9}", s.name), name),
                Span::styled(s.blurb, blurb),
                Span::styled(" ".repeat(pad), Style::default().bg(bg)),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    /// Active section: rows in a base-colored block titled with the
    /// section name, the cursor row's description in the bottom border.
    fn render_section(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let section = &self.sections[self.section];
        let desc = section.rows.get(self.row).map(Row::desc).unwrap_or("");
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_unfocused))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(
                format!(" {} ", section.name),
                Style::default().fg(sem.subtext0),
            ))
            .title_bottom(Span::styled(
                format!(" {} ", fit(desc, area.width.saturating_sub(4) as usize)),
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (lines, cursor_line) = self.section_lines(inner.width as usize, theme);
        let total = lines.len();
        let height = inner.height as usize;
        // Keep the cursor row visible (long theme lists scroll).
        if let Some(pos) = cursor_line {
            if pos < self.scroll {
                self.scroll = pos;
            } else if pos >= self.scroll + height {
                self.scroll = pos + 1 - height.min(pos + 1);
            }
        }
        self.scroll = self.scroll.min(total.saturating_sub(height));
        let scroll = self.scroll;
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
        scrollbar(frame, area, height, total, scroll, theme);

        // Cursor on the editing field's value.
        if let (Some(input), Some(pos)) = (&self.editing, cursor_line) {
            let text = input.value();
            let head: String = text.chars().take(input.cursor()).collect();
            let x = inner.x + (GUTTER + LABEL) as u16 + head.width() as u16;
            let y = inner.y + (pos - scroll) as u16;
            if y < inner.y + inner.height && x < inner.x + inner.width {
                frame.set_cursor_position((x, y));
            }
        }
    }

    /// Rows of the active section as lines, plus the rendered line
    /// index of the cursor row (for scrolling and the edit cursor).
    fn section_lines(&self, width: usize, theme: &Theme) -> (Vec<Line<'static>>, Option<usize>) {
        let sem = &theme.semantic;
        let value_w = width.saturating_sub(GUTTER + LABEL + 2);
        let mut lines = Vec::new();
        let mut cursor_line = None;
        let mut prev_radio = false;
        for (i, row) in self.sections[self.section].rows.iter().enumerate() {
            // A breath of air between a radio group and the fields
            // that follow it (theme list → path, kinds → command).
            if prev_radio && !matches!(row, Row::Radio { .. }) {
                lines.push(Line::raw(""));
            }
            prev_radio = matches!(row, Row::Radio { .. });
            let selected = i == self.row;
            let bg = if selected { sem.selection_bg } else { sem.base };
            let gutter = Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(sem.border_focused).bg(bg),
            );
            let label = |label: &str| {
                Span::styled(
                    format!("{label:<9}  "),
                    Style::default().fg(sem.subtext0).bg(bg),
                )
            };
            match row {
                Row::Text {
                    label: l,
                    value,
                    placeholder,
                    ..
                } => {
                    // The selected text row shows the live input.
                    let value = if selected {
                        self.editing
                            .as_ref()
                            .map(|e| e.value())
                            .unwrap_or_else(|| value.clone())
                    } else {
                        value.clone()
                    };
                    let value_span = if value.is_empty() && self.editing.is_none() {
                        Span::styled(
                            fit(placeholder, value_w),
                            Style::default().fg(sem.overlay0).bg(bg),
                        )
                    } else {
                        Span::styled(
                            fit(&value, value_w),
                            Style::default()
                                .fg(if selected { sem.selection_fg } else { sem.text })
                                .bg(bg),
                        )
                    };
                    lines.push(Line::from(vec![gutter, label(l), value_span]));
                }
                Row::Bool {
                    label: l, value, ..
                } => {
                    let (dot, word, color) = if *value {
                        ("●", "true", sem.mode_browse)
                    } else {
                        ("○", "false", sem.subtext0)
                    };
                    lines.push(Line::from(vec![
                        gutter,
                        label(l),
                        Span::styled(format!("{dot} {word}"), Style::default().fg(color).bg(bg)),
                    ]));
                }
                Row::Radio { group, option, .. } => {
                    let current = self.group_current(group) == option;
                    let (dot, color, modifier) = if current {
                        ("●", sem.border_focused, Modifier::BOLD)
                    } else {
                        ("○", sem.subtext0, Modifier::empty())
                    };
                    let option_style = if selected {
                        Style::default()
                            .fg(sem.selection_fg)
                            .bg(bg)
                            .add_modifier(modifier)
                    } else {
                        Style::default().fg(color).bg(bg).add_modifier(modifier)
                    };
                    lines.push(Line::from(vec![
                        gutter,
                        Span::styled(format!("{dot} "), Style::default().fg(color).bg(bg)),
                        Span::styled(fit(option, value_w), option_style),
                    ]));
                }
            }
            if selected {
                cursor_line = Some(lines.len() - 1);
            }
        }
        (lines, cursor_line)
    }
}
