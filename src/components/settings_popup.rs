//! Settings popup (`:settings`, plans/0003 §4): section sidebar on the
//! left, the section's rows on the right. Labels sit in a fixed column;
//! values render per kind — text fields show a dim placeholder when
//! empty, booleans are ●/○ dots, and one-of-N groups (themes, provider
//! kind) are radio lists, not cycles. Tab or h/l switch sections,
//! j/k/g/G move between rows, ␣/enter activates (set, toggle, or edit
//! in place — Enter commits, Esc cancels). Committing a theme recolors
//! the popup immediately: a live preview of the working copy. Esc
//! closes; a dirty popup saves config.toml and hot-reloads the theme
//! (provider changes note a restart in their row descriptions).
//!
//! Layout: this file is the popup's state + key handling;
//! `sections.rs` builds the row model from config, `render.rs` draws.

mod render;
mod sections;

use self::sections::{Row, Section, build};
use super::vim_input::{Outcome, VimInput};
use crate::action::Action;
use crate::config::Config;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::path::PathBuf;

pub struct SettingsPopup {
    sections: Vec<Section>,
    section: usize,
    /// Cursor row within the active section.
    row: usize,
    /// Line offset of the rows pane (long theme lists scroll).
    scroll: usize,
    /// Some while a field is being edited.
    editing: Option<VimInput>,
    /// Working copy; commits land here, ApplySettings persists it.
    config: Config,
    dirty: bool,
    /// Palette after a theme commit: the popup previews the working
    /// copy's theme immediately. None = still the app's theme.
    preview: Option<Theme>,
}

impl SettingsPopup {
    /// The palette being live-previewed (after a theme-row commit),
    /// if different from the app's committed theme.
    pub fn preview_theme(&self) -> Option<Theme> {
        self.preview
    }

    pub fn new(config: &Config, themes: Vec<String>) -> Self {
        SettingsPopup {
            sections: build(config, themes),
            section: 0,
            row: 0,
            scroll: 0,
            editing: None,
            config: config.clone(),
            dirty: false,
            preview: None,
        }
    }

    /// Current value of a radio group's key in the working config.
    fn group_current<'a>(&'a self, group: &'a str) -> &'a str {
        match (self.sections[self.section].name, group) {
            ("theme", "name") => self.config.theme.name.as_str(),
            ("provider", "kind") => self.config.provider.kind.as_str(),
            (_, group) => group,
        }
    }

    /// Commit one field into the working config.
    fn commit(&mut self, section: &str, key: &str, value: &str) {
        let before = self.config.clone();
        match (section, key) {
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
            ("provider", "kind") => {
                self.config.provider.kind = value.to_string();
            }
            ("provider", "command") => {
                self.config.provider.command =
                    value.split_whitespace().map(str::to_string).collect();
            }
            _ => {}
        }
        if self.config != before {
            self.dirty = true;
            if (section, key) == ("theme", "name") {
                // Live preview: recolor the popup with the new palette.
                self.preview = Some(Theme::load(&self.config.theme.name));
            }
        }
    }

    /// ␣/enter on the cursor row: set a radio option, toggle a bool,
    /// or start editing text in place.
    fn activate(&mut self) {
        let section = self.sections[self.section].name;
        match self.sections[self.section].rows.get(self.row) {
            Some(Row::Bool { key, value, .. }) => {
                let (key, value) = (*key, !*value);
                self.commit(section, key, &value.to_string());
                if let Some(Row::Bool { value: v, .. }) =
                    self.sections[self.section].rows.get_mut(self.row)
                {
                    *v = value;
                }
            }
            Some(Row::Radio { group, option, .. }) => {
                let (group, option) = (*group, option.clone());
                self.commit(section, group, &option);
            }
            Some(Row::Text { value, .. }) => {
                // Transient: Esc stops editing directly, no NORMAL
                // sub-mode (same feel as `/` filters).
                let mut input = VimInput::transient();
                input.set(value);
                self.editing = Some(input);
            }
            None => {}
        }
    }

    /// Commit the in-place edit into its row and the working config.
    fn commit_field(&mut self, value: String) {
        let section = self.sections[self.section].name;
        let Some(Row::Text { key, .. }) = self.sections[self.section].rows.get(self.row) else {
            return;
        };
        let key = *key;
        self.commit(section, key, &value);
        if let Some(Row::Text { value: v, .. }) = self.sections[self.section].rows.get_mut(self.row)
        {
            *v = value;
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
                    self.commit_field(value);
                    self.editing = None;
                }
                Outcome::Cancelled => self.editing = None,
                _ => {}
            }
            return Action::Noop;
        }

        let rows = self.sections[self.section].rows.len();
        match key.code {
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
                self.section = (self.section + 1) % self.sections.len();
                self.row = 0;
                self.scroll = 0;
                Action::Noop
            }
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
                self.section = (self.section + self.sections.len() - 1) % self.sections.len();
                self.row = 0;
                self.scroll = 0;
                Action::Noop
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if rows > 0 {
                    self.row = (self.row + 1).min(rows - 1);
                }
                Action::Noop
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.row = self.row.saturating_sub(1);
                Action::Noop
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.row = 0;
                Action::Noop
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.row = rows.saturating_sub(1);
                Action::Noop
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                self.activate();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn popup() -> SettingsPopup {
        SettingsPopup::new(&Config::default(), vec!["gruvbox-dark".into()])
    }

    fn rows(p: &SettingsPopup) -> &[Row] {
        &p.sections[p.section].rows
    }

    #[test]
    fn tab_switches_sections_and_jk_move_rows() {
        let mut p = popup();
        assert_eq!(p.sections[p.section].name, "editor");
        for want in ["theme", "cache", "provider", "editor"] {
            p.handle_key(key(KeyCode::Tab));
            assert_eq!(p.sections[p.section].name, want);
        }
        p.handle_key(key(KeyCode::BackTab));
        assert_eq!(p.sections[p.section].name, "provider");
        assert_eq!(p.row, 0); // section switch resets the cursor

        p.handle_key(key(KeyCode::Char('j')));
        assert_eq!(p.row, 1);
        p.handle_key(key(KeyCode::Char('G')));
        assert_eq!(p.row, rows(&p).len() - 1);
        p.handle_key(key(KeyCode::Char('g')));
        assert_eq!(p.row, 0);
    }

    #[test]
    fn space_toggles_bools_and_commits() {
        let mut p = popup();
        p.handle_key(key(KeyCode::Char('j')));
        p.handle_key(key(KeyCode::Char('j')));
        p.handle_key(key(KeyCode::Char(' '))); // read_only: true → false
        assert!(matches!(rows(&p)[2], Row::Bool { value: false, .. }));
        assert!(p.dirty);
        assert!(!p.config.editor.read_only);
    }

    #[test]
    fn themes_are_a_radio_list_and_selecting_commits() {
        let mut p = popup();
        p.handle_key(key(KeyCode::Tab)); // theme section
        // Radio rows: one per theme, then the path field.
        assert!(matches!(&rows(&p)[0], Row::Radio { option, .. } if option == "catppuccin-mocha"));
        assert!(matches!(&rows(&p)[1], Row::Radio { option, .. } if option == "gruvbox-dark"));
        assert!(matches!(rows(&p)[2], Row::Text { .. }));

        // Activating the already-current option is a no-op (no dirty).
        p.handle_key(key(KeyCode::Enter));
        assert!(!p.dirty);

        p.handle_key(key(KeyCode::Char('j')));
        p.handle_key(key(KeyCode::Char(' '))); // select gruvbox-dark
        assert_eq!(p.config.theme.name, "gruvbox-dark");
        assert!(p.dirty);
        assert!(
            p.preview.is_some(),
            "theme commit must set the live preview"
        );
    }

    #[test]
    fn provider_tab_selects_kind_and_edits_command() {
        let mut p = popup();
        for _ in 0..3 {
            p.handle_key(key(KeyCode::Tab));
        }
        assert_eq!(p.sections[p.section].name, "provider");

        p.handle_key(key(KeyCode::Char('j')));
        p.handle_key(key(KeyCode::Char(' '))); // github → stdio
        assert_eq!(p.config.provider.kind, "stdio");

        p.handle_key(key(KeyCode::Char('j')));
        p.handle_key(key(KeyCode::Enter)); // edit command (after the group)
        assert!(p.editing.is_some());
        for c in "python3 /tmp/p.py".chars() {
            p.handle_key(key(KeyCode::Char(c)));
        }
        p.handle_key(key(KeyCode::Enter)); // commit
        assert_eq!(p.config.provider.command, vec!["python3", "/tmp/p.py"]);
        // Dirty popup emits ApplySettings on Esc.
        assert!(matches!(
            p.handle_key(key(KeyCode::Esc)),
            Action::ApplySettings(_)
        ));
    }

    #[test]
    fn enter_edits_and_esc_closes() {
        let mut p = popup();
        p.handle_key(key(KeyCode::Enter));
        assert!(p.editing.is_some());
        assert_eq!(p.effective_mode(), Mode::Insert);
        p.handle_key(key(KeyCode::Esc)); // stop editing
        assert!(p.editing.is_none());
        assert_eq!(p.handle_key(key(KeyCode::Esc)), Action::ClosePopup);
    }

    /// Snapshot of the important states: sidebar + rows + radio dots +
    /// placeholders + the unsaved chip (skill: TestBackend per
    /// component).
    #[test]
    fn render_shows_sidebar_radios_placeholders_and_dirty_chip() {
        let mut p = popup();
        let theme = Theme::catppuccin_mocha();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| p.render(f, f.area(), &theme)).unwrap();
        let screen = {
            let buf = terminal.backend().buffer();
            (0..buf.area.height)
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol().to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(screen.contains("▸ editor"), "active section marker");
        assert!(screen.contains("theme"), "sidebar lists theme");
        assert!(
            screen.contains("auto — $VISUAL"),
            "empty text shows placeholder"
        );
        assert!(screen.contains("● true"), "bool renders as a dot");
        assert!(!screen.contains("unsaved"), "clean popup has no dirty chip");

        // Theme section: radio list; select the second theme → chip.
        p.handle_key(key(KeyCode::Tab));
        p.handle_key(key(KeyCode::Char('j')));
        p.handle_key(key(KeyCode::Char(' ')));
        terminal.draw(|f| p.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            screen.contains("○ catppuccin-mocha"),
            "deselected option is a hollow dot"
        );
        assert!(
            screen.contains("● gruvbox-dark"),
            "selected option is a filled dot"
        );
        assert!(screen.contains("unsaved"), "dirty chip appears");
    }
}
