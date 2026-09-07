use super::*;
use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

#[test]
fn types_in_insert_then_esc_to_normal() {
    let mut input = VimInput::new();
    input.handle_key(key(KeyCode::Char('a')));
    input.handle_key(key(KeyCode::Char('b')));
    assert_eq!(input.value(), "ab");
    assert_eq!(input.submode, SubMode::Insert);

    input.handle_key(key(KeyCode::Esc));
    assert_eq!(input.submode, SubMode::Normal);
    // typing no longer inserts
    input.handle_key(key(KeyCode::Char('z')));
    assert_eq!(input.value(), "ab");
}

#[test]
fn normal_motions_and_x() {
    let mut input = VimInput::new();
    input.set("hello");
    input.handle_key(key(KeyCode::Esc));
    input.handle_key(key(KeyCode::Char('0')));
    assert_eq!(input.cursor(), 0);
    input.handle_key(key(KeyCode::Char('x')));
    assert_eq!(input.value(), "ello");
    input.handle_key(key(KeyCode::Char('$')));
    assert_eq!(input.cursor(), 3);
    input.handle_key(key(KeyCode::Char('h')));
    assert_eq!(input.cursor(), 2);
}

#[test]
fn esc_in_normal_cancels() {
    let mut input = VimInput::new();
    input.handle_key(key(KeyCode::Esc));
    assert_eq!(input.handle_key(key(KeyCode::Esc)), Outcome::Cancelled);
}

#[test]
fn enter_submits_from_insert() {
    let mut input = VimInput::new();
    assert_eq!(input.handle_key(key(KeyCode::Enter)), Outcome::Submitted);
}

#[test]
fn transient_input_esc_cancels_without_normal_mode() {
    let mut input = VimInput::transient();
    input.handle_key(key(KeyCode::Char('x')));
    assert_eq!(input.handle_key(key(KeyCode::Esc)), Outcome::Cancelled);
    assert_eq!(input.submode, SubMode::Insert);
}

#[test]
fn backspace_at_start_is_safe() {
    let mut input = VimInput::new();
    input.handle_key(key(KeyCode::Backspace));
    assert_eq!(input.value(), "");
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

/// Input with `value` typed, switched to NORMAL on the last char.
fn normal_at_end(value: &str) -> VimInput {
    let mut input = VimInput::new();
    input.set(value);
    input.handle_key(key(KeyCode::Esc));
    input
}

#[test]
fn w_e_b_walk_word_runs() {
    let mut i = normal_at_end("foo bar-baz qux");
    // NORMAL cursor starts on the last char; walk back first.
    i.handle_key(key(KeyCode::Char('0')));
    // "foo bar-baz qux"
    //  0123456789…
    i.handle_key(key(KeyCode::Char('w')));
    assert_eq!(i.cursor(), 4, "w → next word");
    i.handle_key(key(KeyCode::Char('w')));
    assert_eq!(i.cursor(), 7, "w → punctuation run");
    i.handle_key(key(KeyCode::Char('w')));
    assert_eq!(i.cursor(), 8, "w → baz");
    i.handle_key(key(KeyCode::Char('b')));
    assert_eq!(i.cursor(), 7, "b → punctuation");
    i.handle_key(key(KeyCode::Char('b')));
    assert_eq!(i.cursor(), 4, "b → bar");
    i.handle_key(key(KeyCode::Char('0')));
    i.handle_key(key(KeyCode::Char('e')));
    assert_eq!(i.cursor(), 2, "e → end of foo");
    i.handle_key(key(KeyCode::Char('e')));
    assert_eq!(i.cursor(), 6, "e → end of bar");
}

#[test]
fn d_word_deletes() {
    let mut i = normal_at_end("foo bar baz");
    i.handle_key(key(KeyCode::Char('0')));
    i.handle_key(key(KeyCode::Char('d')));
    i.handle_key(key(KeyCode::Char('w')));
    assert_eq!(i.value(), "bar baz", "dw deletes word + spaces");
    i.handle_key(key(KeyCode::Char('0')));
    i.handle_key(key(KeyCode::Char('d')));
    i.handle_key(key(KeyCode::Char('e')));
    assert_eq!(i.value(), " baz", "de deletes through word end");
    let mut i = normal_at_end("foo bar baz");
    // NORMAL cursor is on the last char; db from there.
    i.handle_key(key(KeyCode::Char('d')));
    i.handle_key(key(KeyCode::Char('b')));
    assert_eq!(i.value(), "foo bar z", "db deletes the previous word");
    // d then a non-motion cancels silently.
    let mut i = normal_at_end("keep me");
    i.handle_key(key(KeyCode::Char('d')));
    i.handle_key(key(KeyCode::Char('q')));
    assert_eq!(i.value(), "keep me");
}

#[test]
fn dd_clears_and_shift_d_truncates() {
    let mut i = normal_at_end("gone soon");
    i.handle_key(key(KeyCode::Char('d')));
    i.handle_key(key(KeyCode::Char('d')));
    assert_eq!(i.value(), "", "dd clears the line");
    assert_eq!(i.cursor(), 0);

    let mut i = normal_at_end("keep drop");
    i.handle_key(key(KeyCode::Char('0')));
    i.handle_key(key(KeyCode::Char('w'))); // on 'd'
    i.handle_key(key(KeyCode::Char('D')));
    assert_eq!(i.value(), "keep ", "D deletes to end of line");
}

#[test]
fn ctrl_w_deletes_word_back_in_insert() {
    let mut i = VimInput::new();
    i.set("foo bar");
    i.handle_key(ctrl('w'));
    assert_eq!(i.value(), "foo ");
    assert_eq!(i.cursor(), 4);
    // Trailing whitespace goes first, one step at a time.
    let mut i = VimInput::new();
    i.set("foo   ");
    i.handle_key(ctrl('w'));
    assert_eq!(i.value(), "foo");
    i.handle_key(ctrl('w'));
    assert_eq!(i.value(), "");
    // Ctrl+W does NOT type a literal w.
    let mut i = VimInput::new();
    i.handle_key(ctrl('w'));
    assert_eq!(i.value(), "");
}

#[test]
fn ctrl_u_clears_to_line_start() {
    let mut i = VimInput::new();
    i.set("hello world");
    i.handle_key(ctrl('u'));
    assert_eq!(i.value(), "");
    assert_eq!(i.cursor(), 0);
}

#[test]
fn shift_i_inserts_at_line_start() {
    let mut i = normal_at_end("abc");
    i.handle_key(key(KeyCode::Char('I')));
    assert_eq!(i.cursor(), 0);
    assert_eq!(i.submode, SubMode::Insert);
    i.handle_key(key(KeyCode::Char('X')));
    assert_eq!(i.value(), "Xabc");
}
