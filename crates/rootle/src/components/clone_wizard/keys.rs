//! Key handling for the clone wizard: screen walking, the repo
//! checklist, destination browsing, and the `/` filter session.

use super::{Button, CloneWizard, Focus, Screen};
use crate::action::Action;
use crate::components::list_view::{Boundary, FilterOutcome, ListMovement, ScrollMovement};
use crate::keymap::{ListCommand, ListContext, list_command};
use ratatui::crossterm::event::KeyEvent;

impl CloneWizard {
    pub(super) fn key_context(&self) -> ListContext {
        match (self.focus, self.screen) {
            (Focus::Buttons, _) => ListContext::CloneButtons,
            (_, Screen::Repos) => ListContext::CloneRepos,
            (_, Screen::Destination) => ListContext::CloneDestination,
            (_, Screen::Summary) => ListContext::CloneSummary,
        }
    }
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if self.filter.active() {
            if self.filter.handle_key(key) != FilterOutcome::Unchanged {
                self.selection.reset();
                self.destination_selection.reset();
                self.viewport.reset();
            }
            return Action::Noop;
        }
        let command = list_command(self.key_context(), key);
        if command == Some(ListCommand::Filter) {
            self.filter.begin();
            return Action::Noop;
        }
        if command == Some(ListCommand::Cancel) {
            if self.filter.clear() {
                self.selection.reset();
                self.destination_selection.reset();
                self.viewport.reset();
                return Action::Noop;
            }
            return Action::ClosePopup;
        }
        match self.focus {
            Focus::Buttons => match command {
                Some(ListCommand::Back) => self.button = Button::Back,
                Some(ListCommand::Forward) => self.button = Button::Next,
                Some(ListCommand::Focus) => self.focus = Focus::List,
                Some(ListCommand::Accept)
                    if self.screen == Screen::Summary && self.button == Button::Next =>
                {
                    return Action::RunClone {
                        repos: self
                            .checked()
                            .map(|repository| repository.name.clone())
                            .collect(),
                        dest: self.dest.clone(),
                    };
                }
                Some(ListCommand::Accept) => self.go(self.button == Button::Next),
                _ => {}
            },
            Focus::List => match command {
                Some(ListCommand::Next) => self.move_list(ListMovement::Next),
                Some(ListCommand::Previous) => self.move_list(ListMovement::Previous),
                Some(ListCommand::First) => self.move_list(ListMovement::First),
                Some(ListCommand::Last) => self.move_list(ListMovement::Last),
                Some(ListCommand::Focus) => self.focus = Focus::Buttons,
                Some(ListCommand::Toggle) => {
                    if let Some(&source) = self.visible_repos().get(self.selection.selected().get())
                        && let Some((_, checked)) = self.repos.get_mut(source)
                    {
                        *checked = !*checked;
                    }
                }
                Some(ListCommand::Descend) => {
                    if let Some(&source) = self
                        .visible_dest()
                        .get(self.destination_selection.selected().get())
                        && let Some(entry) = self.dest_entries.get(source)
                    {
                        let next = if entry == ".." {
                            self.dest.parent().map(|parent| parent.to_path_buf())
                        } else {
                            Some(self.dest.join(entry))
                        };
                        if let Some(next) = next {
                            self.dest = next;
                            self.refresh_dest();
                        }
                    }
                }
                Some(ListCommand::Parent) => {
                    if let Some(parent) = self.dest.parent().map(|parent| parent.to_path_buf()) {
                        self.dest = parent;
                        self.refresh_dest();
                    }
                }
                Some(ListCommand::Accept) => self.go(true),
                _ => {}
            },
        }
        Action::Noop
    }

    fn move_list(&mut self, movement: ListMovement) {
        match self.screen {
            Screen::Repos => {
                self.selection
                    .advance(movement, self.visible_repos().len(), Boundary::Clamp)
            }
            Screen::Destination => self.destination_selection.advance(
                movement,
                self.visible_dest().len(),
                Boundary::Clamp,
            ),
            Screen::Summary => self.viewport.scroll(match movement {
                ListMovement::Next => ScrollMovement::Down,
                ListMovement::Previous => ScrollMovement::Up,
                ListMovement::First => ScrollMovement::Top,
                ListMovement::Last => ScrollMovement::Bottom,
            }),
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
        self.button = Button::Next;
        self.viewport.reset();
        self.filter.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};
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
            vec![
                rootle_provider::RepoInfo::bare("ratatui/ratatui"),
                rootle_provider::RepoInfo::bare("ratatui/comfy-table"),
            ],
            PathBuf::from("/tmp"),
        )
    }

    /// v1.4 (plans/0014 #1): recently pushed repos sort first;
    /// undated entries keep name order; bare selections slot in by
    /// name among the dated ones.
    #[test]
    fn repos_sort_by_pushed_at_then_name() {
        let w = CloneWizard::new(
            vec![
                rootle_provider::RepoInfo::bare("org/zeta"),
                rootle_provider::RepoInfo {
                    name: "org/old".into(),
                    pushed_at: Some("2026-01-05T09:00:00Z".into()),
                    ..Default::default()
                },
                rootle_provider::RepoInfo {
                    name: "org/fresh".into(),
                    pushed_at: Some("2026-08-20T10:11:12Z".into()),
                    ..Default::default()
                },
                rootle_provider::RepoInfo::bare("org/alpha"),
            ],
            PathBuf::from("/tmp"),
        );
        let order: Vec<&str> = w.repos.iter().map(|(r, _)| r.name.as_str()).collect();
        assert_eq!(order, ["org/fresh", "org/old", "org/alpha", "org/zeta"]);
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
