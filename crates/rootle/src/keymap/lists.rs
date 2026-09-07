//! Popup/list bindings. Each surface dispatches these commands and renders
//! the same table's hints; focus changes choose a different table.

use super::{Binding, Key, Table};
use ratatui::crossterm::event::KeyCode;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListCommand {
    Next,
    Previous,
    First,
    Last,
    NextSection,
    PreviousSection,
    Accept,
    Toggle,
    Filter,
    Cancel,
    Focus,
    Parent,
    Descend,
    Back,
    Forward,
}
#[derive(Debug, Clone, Copy)]
pub enum ListContext {
    Refs,
    Help,
    Settings,
    CloneRepos,
    CloneDestination,
    CloneSummary,
    CloneButtons,
}
use ListCommand::*;

fn navigation() -> Vec<Binding<ListCommand>> {
    vec![
        Binding::new(
            "j/k",
            "move",
            Next,
            &[Key::Code(KeyCode::Char('j')), Key::Code(KeyCode::Down)],
        ),
        Binding::new(
            "j/k",
            "move",
            Previous,
            &[Key::Code(KeyCode::Char('k')), Key::Code(KeyCode::Up)],
        ),
        Binding::new(
            "g/G",
            "first/last",
            First,
            &[Key::Code(KeyCode::Char('g')), Key::Code(KeyCode::Home)],
        ),
        Binding::new(
            "g/G",
            "first/last",
            Last,
            &[Key::Code(KeyCode::Char('G')), Key::Code(KeyCode::End)],
        ),
    ]
}
fn filtered(mut bindings: Vec<Binding<ListCommand>>) -> Table<ListCommand> {
    bindings.push(Binding::new(
        "/",
        "filter",
        Filter,
        &[Key::Code(KeyCode::Char('/'))],
    ));
    bindings.push(Binding::new(
        "esc",
        "back",
        Cancel,
        &[Key::Code(KeyCode::Esc)],
    ));
    Table::new(bindings)
}
static REFS: LazyLock<Table<ListCommand>> = LazyLock::new(|| {
    let mut bindings = navigation();
    bindings.push(Binding::new(
        "enter",
        "switch",
        Accept,
        &[Key::Code(KeyCode::Enter)],
    ));
    filtered(bindings)
});
fn sections(label: &'static str) -> Vec<Binding<ListCommand>> {
    let mut bindings = navigation();
    bindings.extend([
        Binding::new(
            "tab/h/l",
            label,
            NextSection,
            &[
                Key::Code(KeyCode::Tab),
                Key::Code(KeyCode::Char('l')),
                Key::Code(KeyCode::Right),
            ],
        ),
        Binding::new(
            "tab/h/l",
            label,
            PreviousSection,
            &[
                Key::Code(KeyCode::BackTab),
                Key::Code(KeyCode::Char('h')),
                Key::Code(KeyCode::Left),
            ],
        ),
    ]);
    bindings
}
static HELP: LazyLock<Table<ListCommand>> = LazyLock::new(|| {
    let mut bindings = sections("mode");
    bindings.push(Binding::new(
        "q/?",
        "close",
        Cancel,
        &[Key::Code(KeyCode::Char('q')), Key::Code(KeyCode::Char('?'))],
    ));
    filtered(bindings)
});
static SETTINGS: LazyLock<Table<ListCommand>> = LazyLock::new(|| {
    let mut bindings = sections("section");
    bindings.push(Binding::new(
        "␣/enter",
        "change",
        Accept,
        &[Key::Code(KeyCode::Char(' ')), Key::Code(KeyCode::Enter)],
    ));
    filtered(bindings)
});
fn clone_list() -> Vec<Binding<ListCommand>> {
    let mut bindings = navigation();
    bindings.push(Binding::new(
        "tab",
        "buttons",
        Focus,
        &[Key::Code(KeyCode::Tab), Key::Code(KeyCode::BackTab)],
    ));
    bindings
}
static CLONE_REPOS: LazyLock<Table<ListCommand>> = LazyLock::new(|| {
    let mut bindings = clone_list();
    bindings.extend([
        Binding::new("␣", "toggle", Toggle, &[Key::Code(KeyCode::Char(' '))]),
        Binding::new("enter", "next", Accept, &[Key::Code(KeyCode::Enter)]),
    ]);
    filtered(bindings)
});
static CLONE_DESTINATION: LazyLock<Table<ListCommand>> = LazyLock::new(|| {
    let mut bindings = clone_list();
    bindings.extend([
        Binding::new(
            "l/enter",
            "descend",
            Descend,
            &[
                Key::Code(KeyCode::Char('l')),
                Key::Code(KeyCode::Right),
                Key::Code(KeyCode::Enter),
            ],
        ),
        Binding::new(
            "h",
            "parent",
            Parent,
            &[Key::Code(KeyCode::Char('h')), Key::Code(KeyCode::Left)],
        ),
    ]);
    filtered(bindings)
});
static CLONE_SUMMARY: LazyLock<Table<ListCommand>> = LazyLock::new(|| {
    let mut bindings = clone_list();
    bindings.push(Binding::new(
        "esc",
        "cancel",
        Cancel,
        &[Key::Code(KeyCode::Esc)],
    ));
    Table::new(bindings)
});
static CLONE_BUTTONS: LazyLock<Table<ListCommand>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new(
            "h/l",
            "choose",
            Back,
            &[Key::Code(KeyCode::Char('h')), Key::Code(KeyCode::Left)],
        ),
        Binding::new(
            "h/l",
            "choose",
            Forward,
            &[Key::Code(KeyCode::Char('l')), Key::Code(KeyCode::Right)],
        ),
        Binding::new("enter", "activate", Accept, &[Key::Code(KeyCode::Enter)]),
        Binding::new(
            "tab",
            "list",
            Focus,
            &[Key::Code(KeyCode::Tab), Key::Code(KeyCode::BackTab)],
        ),
        Binding::new("esc", "cancel", Cancel, &[Key::Code(KeyCode::Esc)]),
    ])
});

pub(super) fn table(context: ListContext) -> &'static Table<ListCommand> {
    match context {
        ListContext::Refs => &REFS,
        ListContext::Help => &HELP,
        ListContext::Settings => &SETTINGS,
        ListContext::CloneRepos => &CLONE_REPOS,
        ListContext::CloneDestination => &CLONE_DESTINATION,
        ListContext::CloneSummary => &CLONE_SUMMARY,
        ListContext::CloneButtons => &CLONE_BUTTONS,
    }
}
