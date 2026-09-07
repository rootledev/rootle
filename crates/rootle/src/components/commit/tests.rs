use super::*;
use crate::request::CommitGeneration;
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use rootle_provider::{CommitFile, FileStatus};

fn request() -> CommitRequest {
    let mut generation = CommitGeneration::default();
    CommitRequest {
        repository: "owner/repo".into(),
        revision: "提交識別子abcdef".into(),
        generation: generation.tick(),
    }
}
fn detail(request: &CommitRequest) -> CommitDetail {
    CommitDetail {
        sha: request.revision.clone(),
        author: "author\u{1b}[31m".into(),
        date: "2026-09-06".into(),
        message: "subject\n\nbody\n4\n5\n6\n7\nreachable final paragraph".into(),
        parents: Vec::new(),
        truncated: false,
        web_url: Some("https://example.test/commit".into()),
        files: vec![CommitFile {
            path: "源码.rs".into(),
            status: FileStatus::Modified,
            additions: Some(1),
            deletions: Some(1),
            previous_path: None,
            binary: false,
            patch: Some("@@ -1 +1 @@\n-文字(短)\n+文字(広い範囲)\n".into()),
        }],
    }
}
fn frame(view: &mut CommitView, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let theme = Theme::catppuccin_mocha();
    terminal
        .draw(|frame| view.render(frame, frame.area(), &theme))
        .unwrap();
    crate::headless::buffer_text(terminal.backend().buffer())
}

#[test]
fn late_replies_cannot_replace_reopened_or_other_repository_commits() {
    let mut old = request();
    let mut fresh = old.clone();
    fresh.generation.tick();
    let mut view = CommitView::open(fresh.clone());
    view.loaded(&old, detail(&old));
    assert!(frame(&mut view, 60, 15).contains("loading commit"));
    old.generation = fresh.generation;
    old.repository = "other/repo".into();
    view.failed(&old, "wrong error".into());
    assert!(frame(&mut view, 60, 15).contains("loading commit"));
    view.loaded(&fresh, detail(&fresh));
    assert!(frame(&mut view, 60, 15).contains("subject"));
}

#[test]
fn full_message_and_unicode_diff_remain_accessible_on_resizes() {
    let request = request();
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail(&request));
    frame(&mut view, 60, 15);
    view.focus_next();
    frame(&mut view, 60, 8);
    view.move_cursor(ListMovement::Last);
    assert!(frame(&mut view, 60, 8).contains("reachable final paragraph"));
    view.focus_next();
    view.open_delta();
    // Opening parses immediately: movement before any draw is valid.
    view.move_cursor(ListMovement::Last);
    for (width, height) in [(1, 1), (2, 2), (3, 4), (12, 6), (80, 15)] {
        frame(&mut view, width, height);
    }
    assert!(frame(&mut view, 80, 15).contains("文字(広い範囲)"));
}

#[test]
fn file_filter_is_live_reversible_and_can_be_reopened_without_old_text() {
    let request = request();
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail(&request));
    view.begin_filter();
    view.filter_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(frame(&mut view, 80, 15).contains("no matching files"));
    view.filter_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(frame(&mut view, 80, 15).contains("源码.rs"));
    view.begin_filter();
    view.filter_key(KeyEvent::new(KeyCode::Char('源'), KeyModifiers::NONE));
    view.filter_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!view.escape());
    view.begin_filter();
    view.filter_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_eq!(view.filter.text(), "x");
}
