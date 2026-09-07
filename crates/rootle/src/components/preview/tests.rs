use super::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent {
        modifiers: KeyModifiers::CONTROL,
        ..key(code)
    }
}

/// 20 lines; blanks at 5 and 12; `fn main() {` on line 7, its `}`
/// on line 15 (0-based 6 and 14).
fn motion_preview() -> Preview {
    let mut lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
    lines[4] = String::new();
    lines[11] = String::new();
    lines[6] = "fn main() {".into();
    lines[14] = "}".into();
    let body = lines.join("\n");
    let mut p = Preview::new();
    p.set_bytes("a.rs", body.as_bytes());
    p.viewport = 10;
    p
}

fn motions(p: &mut Preview, keys: &str) {
    for c in keys.chars() {
        assert!(p.motion_key(key(KeyCode::Char(c))), "motion {c}");
    }
}

#[test]
fn counts_gg_g_and_pages() {
    let mut p = motion_preview();
    motions(&mut p, "3j");
    assert_eq!(p.line(), Some(4));
    motions(&mut p, "gg");
    assert_eq!(p.line(), Some(1));
    motions(&mut p, "5G");
    assert_eq!(p.line(), Some(5));
    motions(&mut p, "G");
    assert_eq!(p.line(), Some(20));
    // Half page down/up (viewport 10), full page.
    assert!(p.motion_key(ctrl(KeyCode::Char('u'))));
    assert_eq!(p.line(), Some(15));
    assert!(p.motion_key(ctrl(KeyCode::Char('d'))));
    assert_eq!(p.line(), Some(20));
    assert!(p.motion_key(ctrl(KeyCode::Char('b'))));
    assert_eq!(p.line(), Some(10));
    // A dangling count is consumed by the next motion only; a
    // non-motion key resets it.
    for c in "7".chars() {
        p.motion_key(key(KeyCode::Char(c)));
    }
    assert!(!p.motion_key(key(KeyCode::Char('x'))));
    motions(&mut p, "j");
    assert_eq!(p.line(), Some(11));
    // A pending g dies on a non-g key.
    motions(&mut p, "g");
    motions(&mut p, "j");
    assert_eq!(p.line(), Some(12));
    assert_eq!(p.motion_pending, None);
}

#[test]
fn paragraphs_brackets_and_view_positioning() {
    let mut p = motion_preview();
    motions(&mut p, "8G"); // inside the second paragraph
    motions(&mut p, "{");
    assert_eq!(p.line(), Some(5)); // the blank above the paragraph
    motions(&mut p, "}");
    assert_eq!(p.line(), Some(12)); // next blank is line 12
    // % matches across the nested block.
    motions(&mut p, "7G");
    motions(&mut p, "%");
    assert_eq!(p.line(), Some(15));
    motions(&mut p, "%");
    assert_eq!(p.line(), Some(7));
    // zz centers the cursor line.
    motions(&mut p, "15G");
    motions(&mut p, "zz");
    assert_eq!(p.scroll, 9); // 14 - 10/2
    // zt / zb pin it top / bottom.
    motions(&mut p, "zt");
    assert_eq!(p.scroll, 14);
    motions(&mut p, "zb");
    assert_eq!(p.scroll, 5); // 15 - 10
}

#[test]
fn motions_noop_on_cursorless_content() {
    let mut p = Preview::new();
    assert!(!p.motion_key(key(KeyCode::Char('j'))));
    assert!(!p.motion_key(key(KeyCode::Char('G'))));
}
#[test]
fn visual_selects_and_copy_targets_it() {
    let mut p = motion_preview();
    // No visual: the copy target is the cursor line.
    p.move_cursor(1);
    let (text, n) = p.copy_target().unwrap();
    assert_eq!((text.as_str(), n), ("line 2\n", 1));
    assert_eq!(p.visual_range(), None);
    // v anchors; motions extend the range; Y targets it.
    p.toggle_visual();
    assert_eq!(p.visual_range(), Some((2, 2)));
    p.move_cursor(2);
    assert_eq!(p.visual_range(), Some((2, 4)));
    let (text, n) = p.copy_target().unwrap();
    assert_eq!(n, 3);
    assert!(text.starts_with("line 2") && text.ends_with("line 4\n"));
    // Motions move the cursor END of the selection (vim-true):
    // gg from line 4 leaves the anchor at line 2.
    motions(&mut p, "gg");
    assert_eq!(p.visual_range(), Some((1, 2)));
    // Esc ladder: first clear clears the selection.
    assert!(p.clear_visual());
    assert_eq!(p.visual_range(), None);
    assert!(!p.clear_visual());
}

#[test]
fn cursor_walks_and_clamps() {
    let mut p = Preview::new();
    p.set_bytes("a.rs", b"one\ntwo\nthree");
    assert_eq!(p.line(), Some(1));
    p.move_cursor(1);
    p.move_cursor(1);
    assert_eq!(p.line(), Some(3));
    p.move_cursor(1); // clamped at last line
    assert_eq!(p.line(), Some(3));
    p.move_cursor(-10); // clamped at first
    assert_eq!(p.line(), Some(1));
}

#[test]
fn cursorless_content_has_no_line() {
    let mut p = Preview::new();
    p.set_dir("src", vec![]);
    assert_eq!(p.line(), None);
    p.move_cursor(1);
    assert_eq!(p.line(), None);
    p.set_bytes("blob", b"\0\0binary\0");
    assert_eq!(p.line(), None);
    assert_eq!(p.readout(), None);
}

#[test]
fn cursor_resets_on_new_content() {
    let mut p = Preview::new();
    p.set_bytes("a.rs", b"one\ntwo\nthree");
    p.move_cursor(2);
    assert_eq!(p.line(), Some(3));
    p.set_highlighted("b.rs", "rust", vec![Line::from("x")]);
    assert_eq!(p.line(), Some(1));
    assert_eq!(p.readout().as_deref(), Some("1/1"));
}

#[test]
fn scroll_follows_cursor_into_viewport() {
    let mut p = Preview::new();
    p.set_bytes(
        "a.rs",
        (1..=50)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
    );
    for _ in 0..49 {
        p.move_cursor(1);
    }
    p.clamp_scroll(10);
    assert_eq!(p.scroll, 40); // cursor 49 visible in rows 40..50
    p.move_cursor(-49);
    p.clamp_scroll(10);
    assert_eq!(p.scroll, 0);
}

#[test]
fn large_previews_do_not_truncate_cursor_or_scroll_to_u16() {
    let mut preview = Preview::new();
    let source = format!("{}END_OF_LONG_PREVIEW\n", "line\n".repeat(70_000));
    preview.set_bytes("large.txt", source.as_bytes());
    preview.set_cursor_line(70_001);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 8)).unwrap();
    terminal
        .draw(|frame| preview.render(frame, frame.area(), &Theme::catppuccin_mocha()))
        .unwrap();
    assert!(
        crate::headless::buffer_text(terminal.backend().buffer()).contains("END_OF_LONG_PREVIEW")
    );
}
