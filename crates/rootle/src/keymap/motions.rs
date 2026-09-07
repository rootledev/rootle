use super::{Binding, Key, Table};
use ratatui::crossterm::event::KeyCode;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionCommand {
    Down,
    Up,
    Start,
    End,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    ParagraphPrevious,
    ParagraphNext,
    Bracket,
    Align,
    AlignTop,
    AlignBottom,
}

pub(super) static MOTIONS: LazyLock<Table<MotionCommand>> = LazyLock::new(|| {
    Table::new(vec![
        Binding::new(
            "j/k",
            "line",
            MotionCommand::Down,
            &[Key::Code(KeyCode::Char('j')), Key::Code(KeyCode::Down)],
        ),
        Binding::new(
            "j/k",
            "line",
            MotionCommand::Up,
            &[Key::Code(KeyCode::Char('k')), Key::Code(KeyCode::Up)],
        ),
        Binding::new(
            "gg/G",
            "top/bottom",
            MotionCommand::Start,
            &[Key::Code(KeyCode::Char('g'))],
        ),
        Binding::new(
            "gg/G",
            "top/bottom",
            MotionCommand::End,
            &[Key::Code(KeyCode::Char('G'))],
        ),
        Binding::new(
            "^D/^U",
            "½ page",
            MotionCommand::HalfPageDown,
            &[Key::Control('d')],
        ),
        Binding::new(
            "^D/^U",
            "½ page",
            MotionCommand::HalfPageUp,
            &[Key::Control('u')],
        ),
        Binding::new(
            "^F/^B",
            "page",
            MotionCommand::PageDown,
            &[Key::Control('f')],
        ),
        Binding::new("^F/^B", "page", MotionCommand::PageUp, &[Key::Control('b')]),
        Binding::new(
            "{/}",
            "paragraph",
            MotionCommand::ParagraphPrevious,
            &[Key::Code(KeyCode::Char('{'))],
        ),
        Binding::new(
            "{/}",
            "paragraph",
            MotionCommand::ParagraphNext,
            &[Key::Code(KeyCode::Char('}'))],
        ),
        Binding::new(
            "%",
            "match bracket",
            MotionCommand::Bracket,
            &[Key::Code(KeyCode::Char('%'))],
        ),
        Binding::new(
            "zt/zz/zb",
            "align viewport",
            MotionCommand::Align,
            &[Key::Code(KeyCode::Char('z'))],
        ),
        Binding::new(
            "zt/zz/zb",
            "align viewport",
            MotionCommand::AlignTop,
            &[Key::Code(KeyCode::Char('t'))],
        ),
        Binding::new(
            "zt/zz/zb",
            "align viewport",
            MotionCommand::AlignBottom,
            &[Key::Code(KeyCode::Char('b'))],
        ),
    ])
});
