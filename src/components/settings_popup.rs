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

use super::pane::fit;
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
use unicode_width::UnicodeWidthStr;

/// Left gutter before the label: the `▌` selection marker (house style).
const GUTTER: usize = 2;
/// Label column width (longest: `read_only`) plus a gap to the value.
const LABEL: usize = 11;
/// Sidebar column: marker + section name + one-word blurb.
const SIDEBAR: u16 = 20;

#[derive(Debug, Clone)]
enum Row {
    /// Free text; enter edits in place. `placeholder` renders dim while
    /// the value is empty — it says what the empty value resolves to.
    Text {
        key: &'static str,
        label: &'static str,
        value: String,
        placeholder: &'static str,
        desc: &'static str,
    },
    /// true/false; activating toggles the dot.
    Bool {
        key: &'static str,
        label: &'static str,
        value: bool,
        desc: &'static str,
    },
    /// One option of a radio group (`name` = themes, `kind` =
    /// providers): activating commits `option` for the group's key.
    /// The dot marks the group's current value (from the working
    /// config, so it tracks edits live).
    Radio {
        group: &'static str,
        option: String,
        desc: &'static str,
    },
}

impl Row {
    /// Footer description of the row under the cursor.
    fn desc(&self) -> &'static str {
        match self {
            Row::Text { desc, .. } | Row::Bool { desc, .. } | Row::Radio { desc, .. } => desc,
        }
    }
}

/// One settings section: sidebar entry plus its rows.
struct Section {
    name: &'static str,
    blurb: &'static str,
    rows: Vec<Row>,
}

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
    pub fn new(config: &Config, themes: Vec<String>) -> Self {
        let mut names = vec!["catppuccin-mocha".to_string()];
        for t in themes {
            if !names.contains(&t) {
                names.push(t);
            }
        }
        let theme_rows = names
            .into_iter()
            .map(|option| Row::Radio {
                group: "name",
                option,
                desc: "palette: ~/.config/rootle/themes/<name>.toml — missing file falls back to embedded mocha",
            })
            .chain([Row::Text {
                key: "path",
                label: "path",
                value: config
                    .theme
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                placeholder: "by name",
                desc: "explicit palette file; wins over name when set",
            }])
            .collect();

        let sections = vec![
            Section {
                name: "editor",
                blurb: "files",
                rows: vec![
                    Row::Text {
                        key: "program",
                        label: "program",
                        value: config.editor.program.clone().unwrap_or_default(),
                        placeholder: "auto — $VISUAL · $EDITOR · hx…",
                        desc: "editor binary; empty → $VISUAL → $EDITOR → first of hx, nvim, vim, vi",
                    },
                    Row::Text {
                        key: "args",
                        label: "args",
                        value: config.editor.args.join(" "),
                        placeholder: "none",
                        desc: "extra arguments inserted before the file path",
                    },
                    Row::Bool {
                        key: "read_only",
                        label: "read_only",
                        value: config.editor.read_only,
                        desc: "vim family opens with -R; others edit the cache copy — rootle never writes back",
                    },
                ],
            },
            Section {
                name: "theme",
                blurb: "colors",
                rows: theme_rows,
            },
            Section {
                name: "cache",
                blurb: "storage",
                rows: vec![Row::Text {
                    key: "max_mb",
                    label: "max_mb",
                    value: config.cache.max_mb.to_string(),
                    placeholder: "512",
                    desc: "blob cache cap in MiB — least-recently-used blobs are evicted past it",
                }],
            },
            Section {
                name: "provider",
                blurb: "backend",
                rows: vec![
                    Row::Radio {
                        group: "kind",
                        option: "github".into(),
                        desc: "built-in GitHub REST backend — applies after restart",
                    },
                    Row::Radio {
                        group: "kind",
                        option: "stdio".into(),
                        desc: "external child process speaking NDJSON-RPC — applies after restart",
                    },
                    Row::Text {
                        key: "command",
                        label: "command",
                        value: config.provider.command.join(" "),
                        placeholder: "required for stdio",
                        desc: "argv for stdio providers; element 0 is the executable — ignored by github",
                    },
                ],
            },
        ];
        SettingsPopup {
            sections,
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

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // After a theme commit the popup renders with the previewed
        // palette, not the app's.
        let theme = self.preview.unwrap_or(*theme);
        let sem = &theme.semantic;
        let popup = super::centered(area, 72, 62);
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
        super::scrollbar(frame, area, height, total, scroll, theme);

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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

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
