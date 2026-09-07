use super::{Binding, Key, Table};
use crate::action::Action;
use crate::action::Action::*;
use ratatui::crossterm::event::KeyCode::*;
use std::sync::LazyLock;

pub(super) static BROWSING: LazyLock<Table<Action>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new(
            "j/k",
            "move",
            MoveDown,
            &[Key::Code(Char('j')), Key::Code(Down)],
        ),
        Binding::new(
            "j/k",
            "move",
            MoveUp,
            &[Key::Code(Char('k')), Key::Code(Up)],
        ),
        Binding::new(
            "h/l",
            "out/in",
            DrillOut,
            &[Key::Code(Char('h')), Key::Code(Left)],
        ),
        Binding::new(
            "h/l",
            "out/in",
            DrillIn,
            &[Key::Code(Char('l')), Key::Code(Right)],
        ),
        Binding::new(
            "J/K",
            "preview line",
            PreviewLineDown,
            &[Key::Code(Char('J'))],
        ),
        Binding::new(
            "J/K",
            "preview line",
            PreviewLineUp,
            &[Key::Code(Char('K'))],
        ),
        Binding::new("enter", "open", OpenSelected, &[Key::Code(Enter)]),
        Binding::new("/", "filter", EnterSearch, &[Key::Code(Char('/'))]),
        Binding::new("n/N", "match", FindNext, &[Key::Code(Char('n'))]),
        Binding::new("n/N", "match", FindPrev, &[Key::Code(Char('N'))]),
        Binding::new("v", "visual", Visual, &[Key::Code(Char('v'))]),
        Binding::new("␣", "leader", Leader, &[Key::Code(Char(' '))]),
        Binding::new(":", "command", CommandLine, &[Key::Code(Char(':'))]),
        Binding::new("?", "keys", KeybindsPopup, &[Key::Code(Char('?'))]),
        Binding::new("esc", "clear filter", ClearFilter, &[Key::Code(Esc)]),
        Binding::new("q", "quit", Quit, &[Key::Code(Char('q'))]),
    ])
});

pub(super) static VISUAL: LazyLock<Table<Action>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new(
            "j/k",
            "move",
            MoveDown,
            &[Key::Code(Char('j')), Key::Code(Down)],
        ),
        Binding::new(
            "j/k",
            "move",
            MoveUp,
            &[Key::Code(Char('k')), Key::Code(Up)],
        ),
        Binding::new(
            "h/l",
            "out/in",
            DrillOut,
            &[Key::Code(Char('h')), Key::Code(Left)],
        ),
        Binding::new(
            "h/l",
            "out/in",
            DrillIn,
            &[Key::Code(Char('l')), Key::Code(Right)],
        ),
        Binding::new("␣", "select", ToggleSelect, &[Key::Code(Char(' '))]),
        Binding::new(":", "command", CommandLine, &[Key::Code(Char(':'))]),
        Binding::new("?", "keys", KeybindsPopup, &[Key::Code(Char('?'))]),
        Binding::new(
            "v/esc",
            "exit",
            ExitVisual,
            &[Key::Code(Char('v')), Key::Code(Esc)],
        ),
    ])
});

pub(super) static LEADER: LazyLock<Table<Action>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("s", "search", LeaderSearch, &[Key::Code(Char('s'))]),
        Binding::new("f", "find file", LeaderFileFind, &[Key::Code(Char('f'))]),
        Binding::new("g", "grep", LeaderGrep, &[Key::Code(Char('g'))]),
        Binding::new(
            "h",
            "repo history",
            LeaderRepositoryHistory,
            &[Key::Code(Char('h'))],
        ),
        Binding::new("b", "branches", LeaderRefs, &[Key::Code(Char('b'))]),
        Binding::new("p", "preview", LeaderPreview, &[Key::Code(Char('p'))]),
        Binding::new(
            "/",
            "find in file",
            LeaderFindInFile,
            &[Key::Code(Char('/'))],
        ),
        Binding::new("y", "yank url", LeaderYank, &[Key::Code(Char('y'))]),
        Binding::new("c", "clear marks", ClearMarks, &[Key::Code(Char('c'))]),
        Binding::new("d", "del org", DeleteMarked, &[Key::Code(Char('d'))]),
        Binding::new("r", "reload", LeaderReload, &[Key::Code(Char('r'))]),
        Binding::new("q", "quit", LeaderQuit, &[Key::Code(Char('q'))]),
        Binding::new("esc", "back", ClosePopup, &[Key::Code(Esc)]),
    ])
});

pub(super) static HISTORY: LazyLock<Table<Action>> = LazyLock::new(|| history_table(true));
pub(super) static REPOSITORY_HISTORY: LazyLock<Table<Action>> =
    LazyLock::new(|| history_table(false));

fn history_table(file_history: bool) -> Table<Action> {
    let mut bindings = vec![
        Binding::new(
            "j/k",
            "commit",
            HistoryDown,
            &[Key::Code(Char('j')), Key::Code(Down)],
        ),
        Binding::new(
            "j/k",
            "commit",
            HistoryUp,
            &[Key::Code(Char('k')), Key::Code(Up)],
        ),
        Binding::new(
            "enter",
            if file_history {
                "file at commit"
            } else {
                "commit detail"
            },
            HistoryOpen,
            &[Key::Code(Enter)],
        ),
        Binding::new("d", "commit detail", CommitDive, &[Key::Code(Char('d'))]),
    ];
    if file_history {
        bindings.push(Binding::new(
            "y",
            "file permalink",
            HistoryYank,
            &[Key::Code(Char('y'))],
        ));
    }
    bindings.extend([
        Binding::new("/", "filter", HistoryFilterBegin, &[Key::Code(Char('/'))]),
        Binding::new("esc", "back", HistoryClose, &[Key::Code(Esc)]),
    ]);
    Table::new(bindings)
}

pub(super) static PREVIEW: LazyLock<Table<Action>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("/", "find", LeaderFindInFile, &[Key::Code(Char('/'))]),
        Binding::new(":", "goto/command", CommandLine, &[Key::Code(Char(':'))]),
        Binding::new("h", "file history", LeaderHistory, &[Key::Code(Char('h'))]),
        Binding::new("b", "blame", BlameToggle, &[Key::Code(Char('b'))]),
        Binding::new(
            "v",
            "select lines",
            PreviewVisualToggle,
            &[Key::Code(Char('v'))],
        ),
        Binding::new("Y", "copy lines", PreviewCopy, &[Key::Code(Char('Y'))]),
        Binding::new("y", "yank url", LeaderYank, &[Key::Code(Char('y'))]),
        Binding::new("n/N", "match", FindNext, &[Key::Code(Char('n'))]),
        Binding::new("n/N", "match", FindPrev, &[Key::Code(Char('N'))]),
        Binding::new(
            "enter",
            "editor or commit",
            PreviewEnter,
            &[Key::Code(Enter)],
        ),
        Binding::new("␣", "leader", Leader, &[Key::Code(Char(' '))]),
        Binding::new(
            "esc/q",
            "back",
            ExitPreview,
            &[Key::Code(Esc), Key::Code(Char('q'))],
        ),
    ])
});

pub(super) static COMMIT: LazyLock<Table<Action>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new(
            "j/k",
            "move",
            CommitDown,
            &[Key::Code(Char('j')), Key::Code(Down)],
        ),
        Binding::new(
            "j/k",
            "move",
            CommitUp,
            &[Key::Code(Char('k')), Key::Code(Up)],
        ),
        Binding::new(
            "gg/G",
            "top/bottom",
            CommitFirst,
            &[Key::Chord('g', 'g'), Key::Code(Home)],
        ),
        Binding::new(
            "gg/G",
            "top/bottom",
            CommitLast,
            &[Key::Code(Char('G')), Key::Code(End)],
        ),
        Binding::new("enter", "file delta", CommitOpen, &[Key::Code(Enter)]),
        Binding::new(
            "]f/[f",
            "next/prev file",
            CommitStepNext,
            &[Key::Chord(']', 'f')],
        ),
        Binding::new(
            "]f/[f",
            "next/prev file",
            CommitStepPrev,
            &[Key::Chord('[', 'f')],
        ),
        Binding::new("tab", "files/preview", CommitFocus, &[Key::Code(Tab)]),
        Binding::new(
            "h/l",
            "delta columns",
            CommitLeft,
            &[Key::Code(Char('h')), Key::Code(Left)],
        ),
        Binding::new(
            "h/l",
            "delta columns",
            CommitRight,
            &[Key::Code(Char('l')), Key::Code(Right)],
        ),
        Binding::new("Y", "yank commit url", CommitYank, &[Key::Code(Char('Y'))]),
        Binding::new(
            "/",
            "filter files",
            CommitFilterBegin,
            &[Key::Code(Char('/'))],
        ),
        Binding::new(
            "esc/q",
            "back",
            CommitClose,
            &[Key::Code(Esc), Key::Code(Char('q'))],
        ),
    ])
});

// Input lifecycle bindings are consumed by the VimInput state machine.
#[derive(Debug, Clone, Copy)]
pub enum InputControl {
    Submit,
    Escape,
    Insert,
}
pub(super) static FILTER: LazyLock<Table<InputControl>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("enter", "commit", InputControl::Submit, &[Key::Code(Enter)]),
        Binding::new("esc", "cancel", InputControl::Escape, &[Key::Code(Esc)]),
    ])
});
pub(super) static INSERT: LazyLock<Table<InputControl>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("enter", "submit", InputControl::Submit, &[Key::Code(Enter)]),
        Binding::new(
            "esc",
            "normal/cancel",
            InputControl::Escape,
            &[Key::Code(Esc)],
        ),
    ])
});
pub(super) static NORMAL: LazyLock<Table<InputControl>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new("i", "insert", InputControl::Insert, &[Key::Code(Char('i'))]),
        Binding::new("esc", "close", InputControl::Escape, &[Key::Code(Esc)]),
    ])
});
