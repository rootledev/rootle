//! Preview column: sanitized text for files, child listing for dirs.
//! Syntax highlighting lands in milestone 5; text is already sanitized.

use crate::sanitize;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Clone, Default)]
pub enum PreviewContent {
    #[default]
    Empty,
    Text(String),
    DirSummary(Vec<String>),
    Binary { size: usize },
}

pub struct Preview {
    pub content: PreviewContent,
    pub title: String,
}

impl Preview {
    pub fn new() -> Self {
        Preview {
            content: PreviewContent::Empty,
            title: "preview".into(),
        }
    }

    /// Load raw bytes (as fetched from a blob); binary → placeholder.
    pub fn set_bytes(&mut self, name: &str, bytes: &[u8]) {
        self.title = sanitize::sanitize_inline(name);
        self.content = if sanitize::is_binary(bytes) {
            PreviewContent::Binary { size: bytes.len() }
        } else {
            PreviewContent::Text(sanitize::sanitize(bytes))
        };
    }

    pub fn set_dir(&mut self, name: &str, children: Vec<String>) {
        self.title = format!("{}/", sanitize::sanitize_inline(name));
        self.content = PreviewContent::DirSummary(children);
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_unfocused))
            .style(Style::default().bg(sem.base))
            .title(Span::styled(
                format!(" {} ", self.title),
                Style::default().fg(sem.subtext0),
            ));

        let lines: Vec<Line> = match &self.content {
            PreviewContent::Empty => {
                vec![Line::from(Span::styled(
                    "nothing selected",
                    Style::default().fg(sem.overlay0),
                ))]
            }
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
                .map(|c| Line::from(Span::styled(c.clone(), Style::default().fg(sem.text))))
                .collect(),
            PreviewContent::Binary { size } => vec![Line::from(Span::styled(
                format!("binary file · {} bytes", size),
                Style::default().fg(sem.warning),
            ))],
        };

        frame.render_widget(
            Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
            area,
        );
    }
}
