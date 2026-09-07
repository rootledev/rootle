//! Exact Full input versus metadata-only control identity.
use super::full;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use rootle_trace::EventKind;
use serde_json::{Value, json};

/// Key identity for Input records: control keys (Enter/Esc/arrows/
/// function keys) and modifier flags are metadata; printable
/// characters are redacted to `"char"` unless Full capture is on.
pub(crate) fn describe_key_with(full: bool, key: &KeyEvent) -> Value {
    let mut mods = Vec::new();
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        mods.push("shift");
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        mods.push("ctrl");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        mods.push("alt");
    }
    if key.modifiers.contains(KeyModifiers::SUPER) {
        mods.push("super");
    }
    if key.modifiers.contains(KeyModifiers::HYPER) {
        mods.push("hyper");
    }
    if key.modifiers.contains(KeyModifiers::META) {
        mods.push("meta");
    }
    json!({
        "code": code_with(full, key.code),
        "mods": mods,
        "kind": match key.kind {
            KeyEventKind::Press => "press",
            KeyEventKind::Repeat => "repeat",
            KeyEventKind::Release => "release",
        },
    })
}

fn code_with(full: bool, code: KeyCode) -> Value {
    match code {
        // Printable characters are user text: identity only under Full.
        KeyCode::Char(c) => json!(if full {
            c.to_string()
        } else {
            "char".to_owned()
        }),
        KeyCode::F(n) => json!(format!("f{n}")),
        KeyCode::Backspace => json!("backspace"),
        KeyCode::Enter => json!("enter"),
        KeyCode::Left => json!("left"),
        KeyCode::Right => json!("right"),
        KeyCode::Up => json!("up"),
        KeyCode::Down => json!("down"),
        KeyCode::Home => json!("home"),
        KeyCode::End => json!("end"),
        KeyCode::PageUp => json!("pageup"),
        KeyCode::PageDown => json!("pagedown"),
        KeyCode::Tab => json!("tab"),
        KeyCode::BackTab => json!("backtab"),
        KeyCode::Delete => json!("delete"),
        KeyCode::Insert => json!("insert"),
        KeyCode::Esc => json!("esc"),
        KeyCode::Null => json!("null"),
        KeyCode::CapsLock => json!("capslock"),
        KeyCode::ScrollLock => json!("scrolllock"),
        KeyCode::NumLock => json!("numlock"),
        KeyCode::PrintScreen => json!("printscreen"),
        KeyCode::Pause => json!("pause"),
        KeyCode::Menu => json!("menu"),
        KeyCode::KeypadBegin => json!("keypadbegin"),
        KeyCode::Media(_) => json!("media"),
        KeyCode::Modifier(_) => json!("modifier"),
    }
}

/// External key at the common `App::handle_key` boundary.
pub(crate) fn record_key(key: &KeyEvent, mode: &'static str) {
    rootle_trace::record_with(EventKind::Input, || {
        let full = full();
        json!({
            "mode": mode,
            "key": describe_key_with(full, key),
        })
    });
}
