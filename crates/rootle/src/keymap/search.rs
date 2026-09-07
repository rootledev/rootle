use super::{Binding, Key, Table};
use ratatui::crossterm::event::KeyCode::*;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchCommand {
    Next,
    Previous,
    ScrollDown,
    ScrollUp,
    Filter,
    Open,
    Cancel,
    Leader,
    NextField,
    PreviousField,
    Left,
    Right,
    Toggle,
    Visual,
    Copy,
    Yank,
    CommandLine,
    Blame,
    FindNext,
    FindPrevious,
    Fold,
    First,
    Last,
}
#[derive(Debug, Clone, Copy)]
pub enum SearchContext {
    Results,
    Facets,
    File,
    Scope,
    ScopePopup,
    Fields,
}
use SearchCommand::*;

pub(super) static RESULTS: LazyLock<Table<SearchCommand>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("enter", "file", Open, &[Key::Code(Enter)]),
        Binding::new(
            "j/k",
            "hits",
            Next,
            &[Key::Code(Char('j')), Key::Code(Down)],
        ),
        Binding::new(
            "j/k",
            "hits",
            Previous,
            &[Key::Code(Char('k')), Key::Code(Up)],
        ),
        Binding::new("J/K", "scroll", ScrollDown, &[Key::Code(Char('J'))]),
        Binding::new("J/K", "scroll", ScrollUp, &[Key::Code(Char('K'))]),
        Binding::new("/", "filter", Filter, &[Key::Code(Char('/'))]),
        Binding::new("tab", "fields", NextField, &[Key::Code(Tab)]),
        Binding::new(
            "shift-tab",
            "previous field",
            PreviousField,
            &[Key::Code(BackTab)],
        ),
        Binding::new("␣", "leader", Leader, &[Key::Code(Char(' '))]),
        Binding::new("esc", "clear/close", Cancel, &[Key::Code(Esc)]),
    ])
});

pub(super) static FACETS: LazyLock<Table<SearchCommand>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new(
            "h/l",
            "chips",
            SearchCommand::Left,
            &[
                Key::Code(Char('h')),
                Key::Code(ratatui::crossterm::event::KeyCode::Left),
            ],
        ),
        Binding::new(
            "h/l",
            "chips",
            SearchCommand::Right,
            &[
                Key::Code(Char('l')),
                Key::Code(ratatui::crossterm::event::KeyCode::Right),
            ],
        ),
        Binding::new(
            "enter/␣",
            "toggle",
            Toggle,
            &[Key::Code(Enter), Key::Code(Char(' '))],
        ),
        Binding::new("tab", "fields", NextField, &[Key::Code(Tab)]),
        Binding::new(
            "shift-tab",
            "previous field",
            PreviousField,
            &[Key::Code(BackTab)],
        ),
        Binding::new("esc", "clear/close", Cancel, &[Key::Code(Esc)]),
    ])
});

pub(super) static FILE: LazyLock<Table<SearchCommand>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("v", "select lines", Visual, &[Key::Code(Char('v'))]),
        Binding::new("Y", "copy lines", Copy, &[Key::Code(Char('Y'))]),
        Binding::new("y", "yank url", Yank, &[Key::Code(Char('y'))]),
        Binding::new("b", "blame", Blame, &[Key::Code(Char('b'))]),
        Binding::new(":", "goto/command", CommandLine, &[Key::Code(Char(':'))]),
        Binding::new("enter", "open", Open, &[Key::Code(Enter)]),
        Binding::new("/", "find", Filter, &[Key::Code(Char('/'))]),
        Binding::new("n/N", "match", FindNext, &[Key::Code(Char('n'))]),
        Binding::new("n/N", "match", FindPrevious, &[Key::Code(Char('N'))]),
        Binding::new("J/K", "line", Next, &[Key::Code(Char('J'))]),
        Binding::new("J/K", "line", Previous, &[Key::Code(Char('K'))]),
        Binding::new("esc/h", "results", Cancel, &[Key::Code(Esc)]),
        Binding::new("esc/h", "results", Fold, &[Key::Code(Char('h'))]),
        Binding::new("␣", "leader", Leader, &[Key::Code(Char(' '))]),
        Binding::new("tab", "fields", NextField, &[Key::Code(Tab)]),
        Binding::new(
            "shift-tab",
            "previous field",
            PreviousField,
            &[Key::Code(BackTab)],
        ),
    ])
});

static SCOPE: LazyLock<Table<SearchCommand>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new(
            "enter/␣",
            "choose scope",
            Open,
            &[Key::Code(Enter), Key::Code(Char(' '))],
        ),
        Binding::new(
            "j/k",
            "scope",
            Next,
            &[Key::Code(Char('j')), Key::Code(Down)],
        ),
        Binding::new(
            "j/k",
            "scope",
            Previous,
            &[Key::Code(Char('k')), Key::Code(Up)],
        ),
        Binding::new("esc", "close", Cancel, &[Key::Code(Esc)]),
    ])
});
static SCOPE_POPUP: LazyLock<Table<SearchCommand>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("enter", "commit", Open, &[Key::Code(Enter)]),
        Binding::new(
            "j/k",
            "scope",
            Next,
            &[Key::Code(Char('j')), Key::Code(Down)],
        ),
        Binding::new(
            "j/k",
            "scope",
            Previous,
            &[Key::Code(Char('k')), Key::Code(Up)],
        ),
        Binding::new("g/G", "first/last", First, &[Key::Code(Char('g'))]),
        Binding::new("g/G", "first/last", Last, &[Key::Code(Char('G'))]),
        Binding::new("esc", "revert", Cancel, &[Key::Code(Esc)]),
    ])
});
static FIELDS: LazyLock<Table<SearchCommand>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("tab", "next field", NextField, &[Key::Code(Tab)]),
        Binding::new(
            "shift-tab",
            "previous field",
            PreviousField,
            &[Key::Code(BackTab)],
        ),
    ])
});

pub(super) fn table(context: SearchContext) -> &'static Table<SearchCommand> {
    match context {
        SearchContext::Results => &RESULTS,
        SearchContext::Facets => &FACETS,
        SearchContext::File => &FILE,
        SearchContext::Scope => &SCOPE,
        SearchContext::ScopePopup => &SCOPE_POPUP,
        SearchContext::Fields => &FIELDS,
    }
}
