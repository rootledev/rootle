//! Keymap tables: mode → key → Action, plus the hint rows derived from
//! the same source (PLAN.md §6). Dispatch and hints can never drift apart.

use crate::action::Action;
use crate::mode::Mode;
use ratatui::crossterm::event::KeyCode;

/// (key label, description) shown in the modeline / popup hint row.
pub fn hints(mode: Mode) -> &'static [(&'static str, &'static str)] {
    match mode {
        Mode::Browse => &[
            ("j/k", "move"),
            ("h/l", "out/in"),
            ("J/K", "preview line"),
            ("/", "filter"),
            ("n/N", "match"),
            ("v", "visual"),
            ("␣", "leader"),
            (":", "command"),
            ("?", "keys"),
            ("q", "quit"),
        ],
        Mode::Search => &[("type", "filter"), ("enter", "commit"), ("esc", "cancel")],
        Mode::Find => &[("type", "query"), ("enter", "jump"), ("esc", "cancel")],
        Mode::Insert => &[("enter", "submit"), ("tab", "results"), ("esc", "normal")],
        Mode::Normal => &[("i", "insert"), ("tab", "switch"), ("esc", "close")],
        Mode::Leader => &[
            ("s", "search"),
            ("f", "find file"),
            ("g", "grep"),
            ("b", "branches"),
            ("h", "history"),
            ("/", "find in file"),
            ("y", "yank url"),
            ("c", "clear marks"),
            ("d", "del org"),
            ("r", "reload"),
            ("q", "quit"),
            ("esc", "back"),
        ],
        Mode::Visual => &[
            ("j/k", "move"),
            ("␣", "select"),
            ("h/l", "out/in"),
            (":", "command"),
            ("v", "exit"),
        ],
        Mode::History => &[
            ("j/k", "commit"),
            ("enter", "file at commit"),
            ("esc", "back"),
        ],
    }
}

/// File-history lens (plans/0016 M1b): the preview pane lists commits.
pub fn history(code: KeyCode) -> Action {
    match code {
        KeyCode::Char('j') | KeyCode::Down => Action::HistoryDown,
        KeyCode::Char('k') | KeyCode::Up => Action::HistoryUp,
        KeyCode::Enter => Action::HistoryOpen,
        KeyCode::Esc => Action::HistoryClose,
        _ => Action::Noop,
    }
}

/// Browsing-mode dispatch. Text input modes are owned by `VimInput`;
/// SEARCHING-mode keys are owned by the pane filter input.
pub fn browsing(code: KeyCode) -> Action {
    match code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char(' ') => Action::Leader,
        KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
        KeyCode::Char('J') => Action::PreviewLineDown,
        KeyCode::Char('K') => Action::PreviewLineUp,
        KeyCode::Char('l') | KeyCode::Right => Action::DrillIn,
        KeyCode::Char('h') | KeyCode::Left => Action::DrillOut,
        KeyCode::Char('/') => Action::EnterSearch,
        KeyCode::Char('n') => Action::FindNext,
        KeyCode::Char('N') => Action::FindPrev,
        KeyCode::Char('v') => Action::Visual,
        KeyCode::Char('?') => Action::KeybindsPopup,
        KeyCode::Char(':') => Action::CommandLine,
        KeyCode::Enter => Action::OpenSelected,
        KeyCode::Esc => Action::ClearFilter,
        _ => Action::Noop,
    }
}

/// VISUAL mode (plans/0004 §1): multi-select with checkboxes.
pub fn visual(code: KeyCode) -> Action {
    match code {
        KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
        KeyCode::Char('h') | KeyCode::Left => Action::DrillOut,
        KeyCode::Char('l') | KeyCode::Right => Action::DrillIn,
        KeyCode::Char(' ') => Action::ToggleSelect,
        KeyCode::Char(':') => Action::CommandLine,
        KeyCode::Char('?') => Action::KeybindsPopup,
        KeyCode::Char('v') | KeyCode::Esc => Action::ExitVisual,
        _ => Action::Noop,
    }
}

pub fn leader(code: KeyCode) -> Action {
    match code {
        KeyCode::Char('s') => Action::LeaderSearch,
        KeyCode::Char('f') => Action::LeaderFileFind,
        KeyCode::Char('g') => Action::LeaderGrep,
        KeyCode::Char('/') => Action::LeaderFindInFile,
        KeyCode::Char('y') => Action::LeaderYank,
        KeyCode::Char('b') => Action::LeaderRefs,
        KeyCode::Char('h') => Action::LeaderHistory,
        KeyCode::Char('c') => Action::ClearMarks,
        KeyCode::Char('d') => Action::DeleteMarked,
        KeyCode::Char('r') => Action::LeaderReload,
        KeyCode::Char('q') => Action::LeaderQuit,
        KeyCode::Esc => Action::ClosePopup, // reused: "back to previous mode"
        _ => Action::Noop,
    }
}

/// Global search view (plans/0012 M2): the view owns dispatch — its
/// state decides what a key means (fields, scope popup, results,
/// expanded file pane) — so these rows are hint-source, not dispatch.
/// They live with the other tables so the view's hint row can never
/// drift from the keys `keys.rs` actually matches.
pub fn search_results() -> &'static [(&'static str, &'static str)] {
    &[
        ("enter", "file"),
        ("j/k", "hits"),
        ("/", "filter"),
        ("tab", "fields"),
        ("esc", "close"),
    ]
}

/// The facet chip row (plans/0012 M3), reached by `tab` once results
/// stream in: h/l walk chips, Enter/Space toggles the chip under the
/// cursor — the active chip is the committed filter over the
/// accumulated set; toggling it again restores everything.
pub fn search_facets() -> &'static [(&'static str, &'static str)] {
    &[
        ("h/l", "chips"),
        ("enter", "toggle"),
        ("tab", "fields"),
        ("esc", "clear/close"),
    ]
}

/// The expanded full-file pane (`Enter` on a hit): j/k walk lines,
/// `Enter` opens the editor, `/` finds in the file, `Esc`/`h` folds
/// back to the results list.
pub fn search_file() -> &'static [(&'static str, &'static str)] {
    &[
        ("j/k", "lines"),
        ("enter", "open"),
        ("/", "find"),
        ("n/N", "match"),
        ("esc/h", "results"),
    ]
}

/// Hint-row text for a table: ` k1 d1 · k2 d2 · … ` — the same rows
/// the modeline and `?` popup render, packed into a border title.
pub fn hint_row(rows: &[(&'static str, &'static str)]) -> String {
    let body = rows
        .iter()
        .map(|(key, desc)| format!("{key} {desc}"))
        .collect::<Vec<_>>()
        .join(" · ");
    format!(" {body} ")
}
