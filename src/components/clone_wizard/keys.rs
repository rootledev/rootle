//! Key handling for the clone wizard: screen walking, the repo
//! checklist, destination browsing, and the `/` filter session.

use super::{CloneWizard, Focus, Screen};
use crate::action::Action;
use crate::components::vim_input::Outcome;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

impl CloneWizard {
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
                    if let Some(&orig) = self.visible_repos().get(self.cursor)
                        && let Some((_, on)) = self.repos.get_mut(orig)
                    {
                        *on = !*on;
                    }
                    Action::Noop
                }
                // Destination browsing: l/Enter descend, h goes up.
                KeyCode::Char('l') | KeyCode::Enter | KeyCode::Right
                    if self.screen == Screen::Destination =>
                {
                    if let Some(&orig) = self.visible_dest().get(self.dest_cursor)
                        && let Some(entry) = self.dest_entries.get(orig)
                    {
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

    fn set_filter(&mut self, value: String) {
        self.filter_value = value;
        self.cursor = 0;
        self.dest_cursor = 0;
        self.scroll = 0;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};
    use std::path::PathBuf;

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
