//! Binding tables are both the dispatcher and the help source. Aliases,
//! modifiers and two-key sequences are described once; contexts supply
//! typed commands to their state machines.

mod browse;
mod lists;
mod motion_state;
mod motions;
mod search;

use crate::{action::Action, mode::Mode};
pub use lists::{ListCommand, ListContext};
pub use motion_state::{MotionCount, MotionPrefix};
pub use motions::MotionCommand;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub use search::{SearchCommand, SearchContext};
use std::sync::LazyLock;

pub type Hint = (&'static str, &'static str);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Code(KeyCode),
    Control(char),
    Chord(char, char),
}
impl Key {
    fn matches(self, key: KeyEvent) -> bool {
        match self {
            Self::Code(code) => {
                key.code == code
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            }
            Self::Control(character) => {
                key.code == KeyCode::Char(character)
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
            }
            Self::Chord(_, _) => false,
        }
    }
}

pub struct Binding<Command: 'static> {
    pub label: &'static str,
    pub description: &'static str,
    pub keys: &'static [Key],
    pub command: Command,
}

impl<Command: 'static> Binding<Command> {
    pub const fn new(
        label: &'static str,
        description: &'static str,
        command: Command,
        keys: &'static [Key],
    ) -> Self {
        Self {
            label,
            description,
            command,
            keys,
        }
    }
}

pub struct Table<Command: 'static> {
    bindings: Vec<Binding<Command>>,
    hints: Vec<Hint>,
}
impl<Command: Clone> Table<Command> {
    pub fn new(bindings: Vec<Binding<Command>>) -> Self {
        let mut hints = Vec::new();
        for binding in &bindings {
            let hint = (binding.label, binding.description);
            if !hints.contains(&hint) {
                hints.push(hint);
            }
        }
        Self { bindings, hints }
    }
    pub fn lookup(&self, key: KeyEvent) -> Option<Command> {
        self.bindings
            .iter()
            .find(|binding| binding.keys.iter().any(|pattern| pattern.matches(key)))
            .map(|binding| binding.command.clone())
    }
    pub fn hints(&'static self) -> &'static [Hint] {
        &self.hints
    }
}

/// A component-owned two-key prefix. Esc cancels an unfinished chord;
/// an invalid second key is consumed, never reinterpreted as a command.
#[derive(Debug, Default)]
pub struct KeySequence {
    pending: Option<char>,
}
impl KeySequence {
    pub fn clear(&mut self) {
        self.pending = None;
    }
    pub fn dispatch<Command: Clone>(
        &mut self,
        table: &Table<Command>,
        key: KeyEvent,
    ) -> Option<Command> {
        if let Some(prefix) = self.pending.take() {
            return table.bindings.iter().find(|binding| binding.keys.iter().any(|pattern| {
                matches!(pattern, Key::Chord(first, second) if *first == prefix && key.code == KeyCode::Char(*second)
                    && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
            })).map(|binding| binding.command.clone());
        }
        if let Some(command) = table.lookup(key) {
            return Some(command);
        }
        if let KeyCode::Char(character) = key.code
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && table.bindings.iter().any(|binding| {
                binding
                    .keys
                    .iter()
                    .any(|pattern| matches!(pattern, Key::Chord(first, _) if *first == character))
            })
        {
            self.pending = Some(character);
        }
        None
    }
}

fn plain(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
pub fn browsing(code: KeyCode) -> Action {
    browse::BROWSING.lookup(plain(code)).unwrap_or(Action::Noop)
}
pub fn visual(code: KeyCode) -> Action {
    browse::VISUAL.lookup(plain(code)).unwrap_or(Action::Noop)
}
pub fn leader(code: KeyCode) -> Action {
    browse::LEADER.lookup(plain(code)).unwrap_or(Action::Noop)
}
pub fn history(code: KeyCode) -> Action {
    browse::HISTORY.lookup(plain(code)).unwrap_or(Action::Noop)
}
pub fn preview_named(code: KeyCode) -> Action {
    browse::PREVIEW.lookup(plain(code)).unwrap_or(Action::Noop)
}
pub fn commit(key: KeyEvent, sequence: &mut KeySequence) -> Action {
    sequence
        .dispatch(&browse::COMMIT, key)
        .unwrap_or(Action::Noop)
}
pub fn motion(key: KeyEvent) -> Option<MotionCommand> {
    motions::MOTIONS.lookup(key)
}
pub fn search_command(context: SearchContext, key: KeyEvent) -> Option<SearchCommand> {
    search::table(context).lookup(key)
}

pub fn list_command(context: ListContext, key: KeyEvent) -> Option<ListCommand> {
    lists::table(context).lookup(key)
}
pub fn list_hints(context: ListContext, filtering: bool) -> &'static [Hint] {
    if filtering {
        browse::FILTER.hints()
    } else {
        lists::table(context).hints()
    }
}

pub use browse::InputControl;
pub fn input_control(key: KeyEvent, normal: bool, modal: bool) -> Option<InputControl> {
    if normal {
        browse::NORMAL.lookup(key)
    } else if modal {
        browse::INSERT.lookup(key)
    } else {
        browse::FILTER.lookup(key)
    }
}

static PREVIEW_HINTS: LazyLock<Vec<Hint>> = LazyLock::new(|| {
    let mut hints = motions::MOTIONS.hints().to_vec();
    hints.extend_from_slice(browse::PREVIEW.hints());
    hints
});

pub fn hints(mode: Mode) -> &'static [Hint] {
    match mode {
        Mode::Browse => browse::BROWSING.hints(),
        Mode::Visual => browse::VISUAL.hints(),
        Mode::Leader => browse::LEADER.hints(),
        Mode::History => browse::HISTORY.hints(),
        Mode::Commit => browse::COMMIT.hints(),
        Mode::Preview => &PREVIEW_HINTS,
        Mode::Search | Mode::Find => browse::FILTER.hints(),
        Mode::Insert => browse::INSERT.hints(),
        Mode::Normal => browse::NORMAL.hints(),
    }
}

pub fn search_results() -> &'static [Hint] {
    search::RESULTS.hints()
}
pub fn search_facets() -> &'static [Hint] {
    search::FACETS.hints()
}
pub fn search_scope_popup() -> &'static [Hint] {
    search::table(SearchContext::ScopePopup).hints()
}
pub fn search_file() -> &'static [Hint] {
    static HINTS: LazyLock<Vec<Hint>> = LazyLock::new(|| {
        let mut hints = motions::MOTIONS.hints().to_vec();
        hints.extend_from_slice(search::FILE.hints());
        hints
    });
    &HINTS
}

pub fn hint_row(rows: &[Hint]) -> String {
    let body = rows
        .iter()
        .map(|(key, description)| format!("{key} {description}"))
        .collect::<Vec<_>>()
        .join(" · ");
    format!(" {body} ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_step_requires_its_whole_chord_and_escape_cancels_prefix() {
        let mut sequence = KeySequence::default();
        assert_eq!(
            commit(plain(KeyCode::Char(']')), &mut sequence),
            Action::Noop
        );
        assert_eq!(
            commit(plain(KeyCode::Char('f')), &mut sequence),
            Action::CommitStepNext
        );
        assert_eq!(
            commit(plain(KeyCode::Char('[')), &mut sequence),
            Action::Noop
        );
        assert_eq!(commit(plain(KeyCode::Esc), &mut sequence), Action::Noop);
        assert_eq!(
            commit(plain(KeyCode::Char('f')), &mut sequence),
            Action::Noop
        );
        assert_eq!(
            commit(plain(KeyCode::Esc), &mut sequence),
            Action::CommitClose
        );
    }
}
