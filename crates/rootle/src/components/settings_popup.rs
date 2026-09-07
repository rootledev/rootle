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
use super::list_view::{Boundary, FilterOutcome, ListCursor, ListFilter, ListMovement, Viewport};
use super::vim_input::{Outcome, VimInput};
use crate::action::Action;
use crate::config::Config;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::KeyEvent;
use std::path::PathBuf;

pub struct SettingsPopup {
    sections: Vec<Section>,
    section: usize,
    selection: ListCursor,
    viewport: Viewport,
    filter: ListFilter,
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
            selection: ListCursor::new(),
            viewport: Viewport::default(),
            filter: ListFilter::default(),
            editing: None,
            config: config.clone(),
            dirty: false,
            preview: None,
        }
    }

    fn visible_rows(&self) -> Vec<usize> {
        self.filter
            .visible(&self.sections[self.section].rows, |row, filter| {
                filter.matches(row.desc())
                    || match row {
                        Row::Text { label, value, .. } => {
                            filter.matches(label) || filter.matches(value)
                        }
                        Row::Bool { label, .. } => filter.matches(label),
                        Row::Radio { group, option, .. } => {
                            filter.matches(group) || filter.matches(option)
                        }
                    }
            })
    }

    fn selected_source(&self) -> Option<usize> {
        self.visible_rows()
            .get(self.selection.selected().get())
            .copied()
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
            ("ui", "border") => {
                self.config.ui.border = value.trim().to_string();
            }
            ("ui", "nerd_font") => {
                self.config.ui.nerd_font = value.trim().eq_ignore_ascii_case("true");
            }
            ("ui", "separator") => {
                self.config.ui.separator = value.trim().to_string();
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
        let Some(row_index) = self.selected_source() else {
            return;
        };
        let section = self.sections[self.section].name;
        match self.sections[self.section].rows.get(row_index) {
            Some(Row::Bool { key, value, .. }) => {
                let (key, value) = (*key, !*value);
                self.commit(section, key, &value.to_string());
                if let Some(Row::Bool { value: v, .. }) =
                    self.sections[self.section].rows.get_mut(row_index)
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
        let Some(row_index) = self.selected_source() else {
            return;
        };
        let section = self.sections[self.section].name;
        let Some(Row::Text { key, .. }) = self.sections[self.section].rows.get(row_index) else {
            return;
        };
        let key = *key;
        self.commit(section, key, &value);
        if let Some(Row::Text { value: v, .. }) =
            self.sections[self.section].rows.get_mut(row_index)
        {
            *v = value;
        }
    }

    /// Modeline chip: INSERT while editing a field, BROWSE otherwise.
    pub fn effective_mode(&self) -> Mode {
        if self.filter.active() {
            Mode::Search
        } else if self.editing.is_some() {
            Mode::Insert
        } else {
            Mode::Browse
        }
    }

    pub fn cursor_style(&self) -> Option<SetCursorStyle> {
        self.editing.as_ref().map(|_| SetCursorStyle::SteadyBar)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        use crate::keymap::{ListCommand, ListContext, list_command};
        if self.filter.active() {
            if self.filter.handle_key(key) != FilterOutcome::Unchanged {
                self.selection.reset();
                self.viewport.reset();
            }
            return Action::Noop;
        }
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

        let rows = self.visible_rows().len();
        match list_command(ListContext::Settings, key) {
            Some(ListCommand::NextSection) => {
                self.section = (self.section + 1) % self.sections.len();
                self.selection.reset();
                self.viewport.reset();
                self.filter.clear();
                Action::Noop
            }
            Some(ListCommand::PreviousSection) => {
                self.section = (self.section + self.sections.len() - 1) % self.sections.len();
                self.selection.reset();
                self.viewport.reset();
                self.filter.clear();
                Action::Noop
            }
            Some(ListCommand::Next) => {
                self.selection
                    .advance(ListMovement::Next, rows, Boundary::Clamp);
                Action::Noop
            }
            Some(ListCommand::Previous) => {
                self.selection
                    .advance(ListMovement::Previous, rows, Boundary::Clamp);
                Action::Noop
            }
            Some(ListCommand::First) => {
                self.selection
                    .advance(ListMovement::First, rows, Boundary::Clamp);
                Action::Noop
            }
            Some(ListCommand::Last) => {
                self.selection
                    .advance(ListMovement::Last, rows, Boundary::Clamp);
                Action::Noop
            }
            Some(ListCommand::Accept) => {
                self.activate();
                Action::Noop
            }
            Some(ListCommand::Filter) => {
                self.filter.begin();
                Action::Noop
            }
            Some(ListCommand::Cancel) if self.filter.clear() => {
                self.selection.reset();
                self.viewport.reset();
                Action::Noop
            }
            Some(ListCommand::Cancel) => {
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
    fn filtered_activation_changes_the_source_setting_and_escape_unwinds() {
        let mut popup = popup();
        popup.handle_key(key(KeyCode::Char('/')));
        for character in "read_only".chars() {
            popup.handle_key(key(KeyCode::Char(character)));
        }
        popup.handle_key(key(KeyCode::Enter));
        popup.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(
            popup.handle_key(key(KeyCode::Esc)),
            Action::Noop,
            "first escape clears filter"
        );
        let Action::ApplySettings(config) = popup.handle_key(key(KeyCode::Esc)) else {
            panic!("edited settings must be committed");
        };
        assert!(
            !config.editor.read_only,
            "filtered index must resolve to read_only, not program"
        );
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
        for _ in 0..4 {
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
