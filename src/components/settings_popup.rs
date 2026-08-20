//! Settings popup (`:settings`, plans/0003 §4): tabs across the top —
//! one per config section — fields below. Tab or h/l switch tabs,
//! j/k move between fields, Enter edits a value in place (Enter
//! commits, Esc stops editing), Esc closes. Stage mock: values come
//! from `Config`, edits stay in memory (write-back + hot reload wire
//! up in the functionality pass).

use super::vim_input::{Outcome, VimInput};
use crate::action::Action;
use crate::config::Config;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::path::PathBuf;

/// One editable field: label + current value as text.
#[derive(Debug, Clone)]
struct Field {
    key: &'static str,
    value: String,
}

pub struct SettingsPopup {
    /// Tab titles = config sections; fields per tab.
    tabs: Vec<(&'static str, Vec<Field>)>,
    tab: usize,
    field: usize,
    /// Some while a field is being edited.
    editing: Option<VimInput>,
    /// Working copy; commits land here, ApplySettings persists it.
    config: Config,
    dirty: bool,
}

impl SettingsPopup {
    pub fn new(config: &Config) -> Self {
        let editor = vec![
            Field {
                key: "program",
                value: config.editor.program.clone().unwrap_or_default(),
            },
            Field {
                key: "args",
                value: config.editor.args.join(" "),
            },
            Field {
                key: "read_only",
                value: config.editor.read_only.to_string(),
            },
        ];
        let theme = vec![
            Field {
                key: "name",
                value: config.theme.name.clone(),
            },
            Field {
                key: "path",
                value: config
                    .theme
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
        ];
        let cache = vec![Field {
            key: "max_mb",
            value: config.cache.max_mb.to_string(),
        }];
        SettingsPopup {
            tabs: vec![("editor", editor), ("theme", theme), ("cache", cache)],
            tab: 0,
            field: 0,
            editing: None,
            config: config.clone(),
            dirty: false,
        }
    }

    /// Commit one field into the working config.
    fn commit(&mut self, tab: &str, key: &str, value: &str) {
        let before = self.config.clone();
        match (tab, key) {
            ("editor", "program") => {
                self.config.editor.program = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                };
            }
            ("editor", "args") => {
                self.config.editor.args = value.split_whitespace().map(str::to_string).collect();
            }
            ("editor", "read_only") => {
                self.config.editor.read_only = value.trim().eq_ignore_ascii_case("true");
            }
            ("theme", "name") => {
                self.config.theme.name = if value.is_empty() {
                    "catppuccin-mocha".into()
                } else {
                    value.to_string()
                };
            }
            ("theme", "path") => {
                self.config.theme.path = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            ("cache", "max_mb") => {
                self.config.cache.max_mb = value.trim().parse().unwrap_or(512);
            }
            _ => {}
        }
        if self.config != before {
            self.dirty = true;
        }
    }

    /// Modeline chip: INSERT while editing a field, BROWSE otherwise.
    pub fn effective_mode(&self) -> Mode {
        if self.editing.is_some() {
            Mode::Insert
        } else {
            Mode::Browse
        }
    }

    pub fn cursor_style(&self) -> Option<SetCursorStyle> {
        self.editing.as_ref().map(|_| SetCursorStyle::SteadyBar)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // An in-place edit captures keys until commit/cancel.
        if let Some(input) = self.editing.as_mut() {
            let (outcome, value) = (input.handle_key(key), input.value());
            match outcome {
                Outcome::Submitted => {
                    let (tab, key) = (self.tabs[self.tab].0, self.tabs[self.tab].1[self.field].key);
                    self.commit(tab, key, &value);
                    self.tabs[self.tab].1[self.field].value = value;
                    self.editing = None;
                }
                Outcome::Cancelled => self.editing = None,
                _ => {}
            }
            return Action::Noop;
        }

        match key.code {
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
                self.tab = (self.tab + 1) % self.tabs.len();
                self.field = 0;
                Action::Noop
            }
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
                self.tab = (self.tab + self.tabs.len() - 1) % self.tabs.len();
                self.field = 0;
                Action::Noop
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.tabs[self.tab].1.len();
                if len > 0 {
                    self.field = (self.field + 1).min(len - 1);
                }
                Action::Noop
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.field = self.field.saturating_sub(1);
                Action::Noop
            }
            KeyCode::Enter => {
                // Transient: Esc stops editing directly, no NORMAL
                // sub-mode (same feel as `/` filters).
                let mut input = VimInput::transient();
                input.set(&self.tabs[self.tab].1[self.field].value);
                self.editing = Some(input);
                Action::Noop
            }
            KeyCode::Esc => {
                // Dirty working copy → persist + hot reload on close.
                if self.dirty {
                    Action::ApplySettings(self.config.clone())
                } else {
                    Action::ClosePopup
                }
            }
            _ => Action::Noop,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let popup = super::centered(area, 60, 50);
        frame.render_widget(Clear, popup);

        let hint = if self.editing.is_some() {
            " enter commit · esc stop editing "
        } else {
            " tab/h/l tabs · j/k fields · enter edit · esc close "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                " settings ",
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(hint, Style::default().fg(sem.hint)));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(inner);

        // Tab bar: [section] chips, active one highlighted. Hand-rolled
        // spans — the Tabs widget defaults to selecting index 0, which
        // would double-highlight the first tab.
        let mut tab_spans = Vec::new();
        for (i, (name, _)) in self.tabs.iter().enumerate() {
            let style = if i == self.tab {
                Style::default()
                    .fg(sem.crust)
                    .bg(sem.border_focused)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(sem.subtext0)
            };
            tab_spans.push(Span::styled(format!(" {name} "), style));
            tab_spans.push(Span::raw(" "));
        }
        frame.render_widget(Paragraph::new(Line::from(tab_spans)), rows[0]);

        // Fields: label left, value right; selected row highlighted,
        // editing row shows the live input.
        let (_, fields) = &self.tabs[self.tab];
        let mut lines = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            let selected = i == self.field;
            let value = if selected {
                match &self.editing {
                    Some(input) => input.value(),
                    None => f.value.clone(),
                }
            } else {
                f.value.clone()
            };
            let style = if selected {
                Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
            } else {
                Style::default().fg(sem.text)
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" {:12}", f.key), Style::default().fg(sem.subtext0)),
                Span::styled(value, style),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), rows[1]);

        // Cursor on the editing field's value.
        if let Some(input) = &self.editing {
            let y = rows[1].y + self.field as u16;
            let x = rows[1].x + 14 + input.cursor() as u16;
            if y < rows[1].y + rows[1].height && x < rows[1].x + rows[1].width {
                frame.set_cursor_position((x, y));
            }
        }
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
    fn tab_switches_sections_and_jk_move_fields() {
        let mut popup = SettingsPopup::new(&Config::default());
        assert_eq!(popup.tabs[popup.tab].0, "editor");
        popup.handle_key(key(KeyCode::Tab));
        assert_eq!(popup.tabs[popup.tab].0, "theme");
        popup.handle_key(key(KeyCode::Tab));
        assert_eq!(popup.tabs[popup.tab].0, "cache");
        popup.handle_key(key(KeyCode::BackTab));
        assert_eq!(popup.tabs[popup.tab].0, "theme");

        popup.handle_key(key(KeyCode::Char('j')));
        assert_eq!(popup.field, 1);
        popup.handle_key(key(KeyCode::Char('k')));
        assert_eq!(popup.field, 0);
    }

    #[test]
    fn enter_edits_and_esc_closes() {
        let mut popup = SettingsPopup::new(&Config::default());
        popup.handle_key(key(KeyCode::Enter));
        assert!(popup.editing.is_some());
        assert_eq!(popup.effective_mode(), Mode::Insert);
        // Esc stops editing, second Esc closes.
        popup.handle_key(key(KeyCode::Esc));
        assert!(popup.editing.is_none());
        assert_eq!(popup.handle_key(key(KeyCode::Esc)), Action::ClosePopup);
    }
}
