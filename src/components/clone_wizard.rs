//! Clone wizard (`:clone`, plans/0004 §2): three screens — repo
//! checkboxes, local destination mini-browser, summary. Mock stage:
//! no git runs; the summary shows what *would* happen. Esc anywhere
//! closes the whole wizard (no partial state).

use super::vim_input::{Outcome, VimInput};
use crate::action::Action;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Repos,
    Destination,
    Summary,
}

/// List or the button row owns the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Buttons,
}

pub struct CloneWizard {
    screen: Screen,
    /// Resolved repos (files already folded to their repo upstream).
    repos: Vec<(String, bool)>,
    cursor: usize,
    focus: Focus,
    /// Button row cursor: 0 = back, 1 = next/clone.
    button: usize,
    /// Destination browser state (screen 2).
    dest: PathBuf,
    dest_entries: Vec<String>,
    dest_cursor: usize,
    /// Line scroll offset of the current screen's content area.
    scroll: usize,
    /// Transient `/` filter (house style: every scanned list filters).
    filter: VimInput,
    filtering: bool,
    pre_filter: String,
    filter_value: String,
}

impl CloneWizard {
    pub fn new(repos: Vec<String>, start: PathBuf) -> Self {
        let repos = repos.into_iter().map(|r| (r, true)).collect();
        let mut wizard = CloneWizard {
            screen: Screen::Repos,
            repos,
            cursor: 0,
            focus: Focus::List,
            button: 1, // next is the default action
            dest: start,
            dest_entries: vec![],
            dest_cursor: 0,
            scroll: 0,
            filter: VimInput::transient(),
            filtering: false,
            pre_filter: String::new(),
            filter_value: String::new(),
        };
        wizard.refresh_dest();
        wizard
    }

    /// Local dirs of the current destination path, `..` first.
    fn refresh_dest(&mut self) {
        let mut entries = vec!["..".to_string()];
        if let Ok(read) = std::fs::read_dir(&self.dest) {
            let mut dirs: Vec<String> = read
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| !name.starts_with('.')) // no dotdirs
                .collect();
            dirs.sort();
            entries.extend(dirs);
        }
        self.dest_entries = entries;
        self.dest_cursor = 0;
    }

    fn checked(&self) -> impl Iterator<Item = &String> {
        self.repos.iter().filter(|(_, on)| *on).map(|(r, _)| r)
    }

    /// Repo indices surviving the committed filter.
    fn visible_repos(&self) -> Vec<usize> {
        let needle = self.filter_value.to_lowercase();
        self.repos
            .iter()
            .enumerate()
            .filter(|(_, (r, _))| needle.is_empty() || r.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    /// Destination indices surviving the committed filter.
    fn visible_dest(&self) -> Vec<usize> {
        let needle = self.filter_value.to_lowercase();
        self.dest_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| needle.is_empty() || e.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    fn set_filter(&mut self, value: String) {
        self.filter_value = value;
        self.cursor = 0;
        self.dest_cursor = 0;
        self.scroll = 0;
    }

    /// Clone target: <dest>/<org>/<repo> — the org level prevents
    /// same-name collisions between orgs.
    fn target(&self, repo: &str) -> std::path::PathBuf {
        self.dest.join(repo)
    }

    fn buttons(&self) -> (&'static str, &'static str) {
        match self.screen {
            Screen::Repos | Screen::Destination => ("back", "next"),
            Screen::Summary => ("back", "clone!"),
        }
    }

    fn go(&mut self, forward: bool) {
        self.screen = match (self.screen, forward) {
            (Screen::Repos, true) => Screen::Destination,
            (Screen::Destination, true) => Screen::Summary,
            (Screen::Destination, false) => Screen::Repos,
            (Screen::Summary, false) => Screen::Destination,
            (s, _) => s, // Repos+back / Summary+forward: stay (mock)
        };
        self.focus = Focus::List;
        self.button = 1;
        self.scroll = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Active `/` session captures everything until commit/cancel.
        if self.filtering {
            return match self.filter.handle_key(key) {
                Outcome::Changed => {
                    self.set_filter(self.filter.value());
                    Action::Noop
                }
                Outcome::Submitted => {
                    self.filtering = false;
                    Action::Noop
                }
                Outcome::Cancelled => {
                    self.set_filter(self.pre_filter.clone());
                    self.filtering = false;
                    Action::Noop
                }
                Outcome::Noop => Action::Noop,
            };
        }
        // `/` filters the repos/destination lists (house style).
        if key.code == KeyCode::Char('/')
            && self.focus == Focus::List
            && self.screen != Screen::Summary
        {
            self.pre_filter = self.filter_value.clone();
            self.filter.set(&self.pre_filter);
            self.filtering = true;
            return Action::Noop;
        }
        // Committed filter? First Esc clears it, second closes.
        if key.code == KeyCode::Esc && !self.filter_value.is_empty() {
            self.set_filter(String::new());
            return Action::Noop;
        }
        // Esc closes the whole wizard from any screen (plans/0004 §2).
        if key.code == KeyCode::Esc {
            return Action::ClosePopup;
        }
        match self.focus {
            Focus::Buttons => match key.code {
                KeyCode::Char('h') | KeyCode::Left => {
                    self.button = 0;
                    Action::Noop
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.button = 1;
                    Action::Noop
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus = Focus::List;
                    Action::Noop
                }
                KeyCode::Enter if self.screen == Screen::Summary && self.button == 1 => {
                    let repos: Vec<String> = self.checked().cloned().collect();
                    Action::RunClone {
                        repos,
                        dest: self.dest.clone(),
                    }
                }
                KeyCode::Enter => {
                    self.go(self.button == 1);
                    Action::Noop
                }
                _ => Action::Noop,
            },
            Focus::List => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    match self.screen {
                        // The summary has no list — j/k scroll it.
                        Screen::Summary => self.scroll += 1,
                        _ => {
                            let len = match self.screen {
                                Screen::Repos => self.visible_repos().len(),
                                _ => self.visible_dest().len(),
                            };
                            if len > 0 {
                                let cursor = match self.screen {
                                    Screen::Repos => &mut self.cursor,
                                    _ => &mut self.dest_cursor,
                                };
                                *cursor = (*cursor + 1).min(len - 1);
                            }
                        }
                    }
                    Action::Noop
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    match self.screen {
                        Screen::Summary => self.scroll = self.scroll.saturating_sub(1),
                        _ => {
                            let cursor = match self.screen {
                                Screen::Repos => &mut self.cursor,
                                _ => &mut self.dest_cursor,
                            };
                            *cursor = cursor.saturating_sub(1);
                        }
                    }
                    Action::Noop
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus = Focus::Buttons;
                    Action::Noop
                }
                KeyCode::Char(' ') if self.screen == Screen::Repos => {
                    if let Some(&orig) = self.visible_repos().get(self.cursor) {
                        if let Some((_, on)) = self.repos.get_mut(orig) {
                            *on = !*on;
                        }
                    }
                    Action::Noop
                }
                // Destination browsing: l/Enter descend, h goes up.
                KeyCode::Char('l') | KeyCode::Enter | KeyCode::Right
                    if self.screen == Screen::Destination =>
                {
                    if let Some(&orig) = self.visible_dest().get(self.dest_cursor) {
                        if let Some(entry) = self.dest_entries.get(orig) {
                            let next = if entry == ".." {
                                self.dest.parent().map(|p| p.to_path_buf())
                            } else {
                                Some(self.dest.join(entry))
                            };
                            if let Some(next) = next {
                                self.dest = next;
                                self.refresh_dest();
                            }
                        }
                    }
                    Action::Noop
                }
                KeyCode::Char('h') | KeyCode::Left if self.screen == Screen::Destination => {
                    if let Some(parent) = self.dest.parent().map(|p| p.to_path_buf()) {
                        self.dest = parent;
                        self.refresh_dest();
                    }
                    Action::Noop
                }
                // Enter anywhere else on the list acts like `next`.
                KeyCode::Enter => {
                    self.go(true);
                    Action::Noop
                }
                _ => Action::Noop,
            },
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let popup = super::centered(area, 70, 60);
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
            .border_type(BorderType::Rounded)
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
        super::scrollbar(frame, popup, height, total, scroll, theme);
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
                let style = if selected {
                    Style::default().fg(sem.selection_fg).bg(sem.selection_bg)
                } else {
                    Style::default().fg(sem.text)
                };
                Line::from(vec![
                    Span::styled(
                        if selected { "▌ " } else { "  " },
                        Style::default().fg(sem.border_focused),
                    ),
                    Span::styled(format!("{mark} "), Style::default().fg(mark_color)),
                    Span::styled(repo.clone(), style),
                ])
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
        let repos: Vec<&String> = self.checked().collect();
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

    fn wizard() -> CloneWizard {
        CloneWizard::new(
            vec!["ratatui/ratatui".into(), "ratatui/comfy-table".into()],
            PathBuf::from("/tmp"),
        )
    }

    #[test]
    fn screens_walk_forward_and_back() {
        let mut w = wizard();
        assert_eq!(w.screen, Screen::Repos);
        w.handle_key(key(KeyCode::Tab)); // list → buttons (on "next")
        w.handle_key(key(KeyCode::Enter));
        assert_eq!(w.screen, Screen::Destination);
        w.handle_key(key(KeyCode::Tab));
        w.handle_key(key(KeyCode::Enter));
        assert_eq!(w.screen, Screen::Summary);
        // Back from summary returns to destination.
        w.handle_key(key(KeyCode::Tab));
        w.handle_key(key(KeyCode::Char('h'))); // next → back
        w.handle_key(key(KeyCode::Enter));
        assert_eq!(w.screen, Screen::Destination);
    }

    #[test]
    fn space_toggles_repos_and_esc_closes_anywhere() {
        let mut w = wizard();
        w.handle_key(key(KeyCode::Char(' ')));
        assert!(!w.repos[0].1);
        assert_eq!(w.checked().count(), 1);
        w.handle_key(key(KeyCode::Tab));
        w.handle_key(key(KeyCode::Enter)); // → destination
        assert_eq!(w.handle_key(key(KeyCode::Esc)), Action::ClosePopup);
    }
    #[test]
    fn slash_filter_narrows_repos_and_destination() {
        let mut w = wizard(); // alpha + comfy-table
        w.handle_key(key(KeyCode::Char('/')));
        for c in "comfy".chars() {
            w.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(w.visible_repos().len(), 1);
        w.handle_key(key(KeyCode::Enter)); // commit
        assert_eq!(w.visible_repos().len(), 1);
        w.handle_key(key(KeyCode::Esc)); // first Esc clears the filter…
        assert_eq!(w.visible_repos().len(), 2);
        // …the second closes the wizard.
        assert_eq!(w.handle_key(key(KeyCode::Esc)), Action::ClosePopup);
    }
}
