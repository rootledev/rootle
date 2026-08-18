//! Bottom modeline: mode chip · context · key hints (PLAN.md §2, §5).

use crate::keymap;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub struct Modeline {
    pub context: String,
}

impl Modeline {
    pub fn new() -> Self {
        Modeline {
            context: String::new(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, mode: Mode, theme: &Theme) {
        let sem = &theme.semantic;
        let chip_bg = match mode {
            Mode::Browsing => sem.mode_browsing,
            Mode::Searching => sem.mode_searching,
            Mode::InputInsert => sem.mode_insert,
            Mode::InputNormal => sem.mode_normal,
            Mode::Leader => sem.mode_leader,
            Mode::Visual => sem.mode_leader,
        };

        let mut spans = vec![
            Span::styled(
                format!(" {} ", mode.chip()),
                Style::default()
                    .fg(sem.crust)
                    .bg(chip_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().bg(sem.mantle)),
            Span::styled(
                format!(" {} ", self.context),
                Style::default().fg(sem.subtext0).bg(sem.mantle),
            ),
        ];

        let hint_text: Vec<Span> = keymap::hints(mode)
            .iter()
            .flat_map(|(k, desc)| {
                [
                    Span::styled(
                        format!(" {k}"),
                        Style::default()
                            .fg(sem.text)
                            .bg(sem.mantle)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" {desc} ·"), Style::default().fg(sem.hint).bg(sem.mantle)),
                ]
                .into_iter()
            })
            .collect();

        let used: usize = spans.iter().map(|s| s.content.width()).sum::<usize>()
            + hint_text.iter().map(|s| s.content.width()).sum::<usize>();
        let pad = (area.width as usize).saturating_sub(used);
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().bg(sem.mantle),
        ));
        spans.extend(hint_text);

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

use unicode_width::UnicodeWidthStr;
