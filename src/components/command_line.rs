//! Command line (`:`, plans/0003 §3): a transient input strip anchored
//! above the modeline, with a small filtered option list popping above
//! it as you type. Enter runs the selected command, Esc cancels.
//! Options derive from `commands::COMMANDS`; arrows move the pick.

use super::vim_input::{Outcome, VimInput};
use crate::action::Action;
use crate::commands;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

pub struct CommandLine {
    pub input: VimInput,
    /// Index into the filtered option list.
    selected: usize,
}

impl CommandLine {
    pub fn new() -> Self {
        CommandLine {
            input: VimInput::transient(),
            selected: 0,
        }
    }

    fn options(&self) -> Vec<&'static commands::Command> {
        commands::filter(&self.input.value())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Option-list navigation wins over text input for arrows.
        match key.code {
            KeyCode::Down => {
                let len = self.options().len();
                if len > 0 {
                    self.selected = (self.selected + 1).min(len - 1);
                }
                return Action::Noop;
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                return Action::Noop;
            }
            _ => {}
        }
        match self.input.handle_key(key) {
            Outcome::Submitted => {
                let options = self.options();
                // The highlighted option wins; a fully typed exact name
                // still runs even if the list cursor is elsewhere.
                match options.get(self.selected) {
                    Some(cmd) => Action::RunCommand(cmd.name.to_string()),
                    None => Action::ClosePopup, // no match: just dismiss
                }
            }
            Outcome::Cancelled => Action::ClosePopup,
            Outcome::Changed => {
                self.selected = 0;
                Action::Noop
            }
            Outcome::Noop => Action::Noop,
        }
    }

    pub fn cursor_style(&self) -> Option<SetCursorStyle> {
        Some(SetCursorStyle::SteadyBar)
    }

    /// Render into `area` (the main region above the modeline): a
    /// one-line strip at the bottom, options floating above it.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let options = self.options();

        // Option list: up to 6 rows + border, above the input strip.
        let shown = options.len().min(6) as u16;
        if shown > 0 {
            let list_area = Rect {
                x: area.x,
                y: area.y + area.height - 1 - shown - 2,
                width: area.width.min(56),
                height: shown + 2,
            };
            frame.render_widget(Clear, list_area);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(sem.border_focused))
                .style(Style::default().bg(sem.mantle));
            let inner = block.inner(list_area);
            frame.render_widget(block, list_area);
            let rows: Vec<Line> = options
                .iter()
                .take(6)
                .enumerate()
                .map(|(i, cmd)| {
                    let picked = i == self.selected;
                    let style = if picked {
                        Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
                    } else {
                        Style::default().fg(sem.text)
                    };
                    Line::from(vec![
                        Span::styled(
                            if picked { "▌" } else { " " },
                            Style::default().fg(sem.border_focused),
                        ),
                        Span::styled(
                            format!("{:10}", cmd.name),
                            style.add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(cmd.summary, Style::default().fg(sem.subtext0)),
                    ])
                })
                .collect();
            frame.render_widget(Paragraph::new(rows), inner);
        }

        // The `:` strip itself, on the modeline's doorstep.
        let strip = Rect {
            x: area.x,
            y: area.y + area.height - 1,
            width: area.width,
            height: 1,
        };
        frame.render_widget(Clear, strip);
        let value = self.input.value();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    ":",
                    Style::default()
                        .fg(sem.mode_leader)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(value.clone(), Style::default().fg(sem.text)),
            ]))
            .style(Style::default().bg(sem.mantle)),
            strip,
        );
        let x = strip.x + 1 + self.input.cursor() as u16;
        if x < strip.x + strip.width {
            frame.set_cursor_position((x, strip.y));
        }
    }
}

impl Default for CommandLine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn typing_filters_and_enter_runs_selected() {
        let mut cl = CommandLine::new();
        for c in "set".chars() {
            assert_eq!(cl.handle_key(key(KeyCode::Char(c))), Action::Noop);
        }
        assert_eq!(
            cl.handle_key(key(KeyCode::Enter)),
            Action::RunCommand("settings".into())
        );
    }

    #[test]
    fn arrows_move_the_pick_and_esc_cancels() {
        let mut cl = CommandLine::new();
        assert_eq!(cl.selected, 0);
        cl.handle_key(key(KeyCode::Down));
        assert_eq!(cl.selected, 1);
        cl.handle_key(key(KeyCode::Up));
        assert_eq!(cl.selected, 0);
        assert_eq!(cl.handle_key(key(KeyCode::Esc)), Action::ClosePopup);
    }

    #[test]
    fn clone_command_runs() {
        let mut cl = CommandLine::new();
        for c in "clone".chars() {
            cl.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            cl.handle_key(key(KeyCode::Enter)),
            Action::RunCommand("clone".into())
        );
    }
}
