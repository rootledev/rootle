use super::actions::describe_action;
use super::events::describe_event;
use super::*;
use crate::{action::Action, event::AppEvent};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Tests run without an active trace session: the default policy
/// is Metadata, so the `*_with(false, ..)` forms pin both sides
/// deterministically.

#[test]
fn printable_characters_are_redacted_in_metadata() {
    let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    let described = describe_key_with(false, &key);
    assert_eq!(described["code"], json!("char"));
    assert!(described.to_string().find('x').is_none());
}

#[test]
fn full_capture_keeps_exact_characters() {
    let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(describe_key_with(true, &key)["code"], json!("x"));
}

#[test]
fn control_key_identity_survives_metadata() {
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
    let described = describe_key_with(false, &key);
    assert_eq!(described["code"], json!("enter"));
    assert_eq!(described["mods"], json!(["ctrl"]));

    let f = KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE);
    assert_eq!(describe_key_with(false, &f)["code"], json!("f5"));
}

#[test]
fn text_is_length_in_metadata_and_exact_in_full() {
    assert_eq!(text_with(false, "hello"), json!(5));
    assert_eq!(text_with(true, "hello"), json!("hello"));
    assert_eq!(opt_text_with(false, &None), Value::Null);
}

#[test]
fn action_query_text_never_reaches_metadata() {
    let action = Action::SearchSubmitted("secret query".into());
    let described = describe_action(&action);
    assert!(!described.to_string().contains("secret"));
    assert_eq!(described["query"], json!("secret query".len()));
}

#[test]
fn action_command_text_is_length_only_in_metadata() {
    let action = Action::RunCommand("settings".into());
    assert!(!describe_action(&action).to_string().contains("settings"));
}

#[test]
fn action_error_is_classified_not_quoted() {
    let action = Action::BlobFailed {
        sha: "abc".into(),
        error: ProviderError::new(rootle_provider::ErrorKind::Auth, "bad credentials"),
    };
    let described = describe_action(&action);
    assert_eq!(described["error"]["kind"], json!("auth"));
    assert!(!described.to_string().contains("bad credentials"));
}

#[test]
fn event_error_is_classified_not_quoted() {
    let event = AppEvent::TreeFailed {
        request: crate::request::TreeRequest {
            repository: "o/r".into(),
            revision: None,
            generation: Default::default(),
        },
        error: ProviderError::other("upstream exploded"),
    };
    let described = describe_event(&event);
    assert_eq!(described["error"]["kind"], json!("other"));
    assert!(!described.to_string().contains("exploded"));
}
