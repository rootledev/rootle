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
    view.loaded(&old, detail(&old), &Theme::catppuccin_mocha());
    assert!(frame(&mut view, 60, 15).contains("loading commit"));
    old.generation = fresh.generation;
    old.repository = "other/repo".into();
    view.failed(&old, "wrong error".into());
    assert!(frame(&mut view, 60, 15).contains("loading commit"));
    view.loaded(&fresh, detail(&fresh), &Theme::catppuccin_mocha());
    assert!(frame(&mut view, 60, 15).contains("subject"));
}

#[test]
fn full_message_and_unicode_diff_remain_accessible_on_resizes() {
    let request = request();
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail(&request), &Theme::catppuccin_mocha());
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
    view.loaded(&request, detail(&request), &Theme::catppuccin_mocha());
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

fn style_at(view: &mut CommitView, theme: &Theme, text: &str) -> ratatui::style::Style {
    let mut terminal = Terminal::new(TestBackend::new(160, 24)).unwrap();
    terminal
        .draw(|frame| view.render(frame, frame.area(), theme))
        .unwrap();
    let buffer = terminal.backend().buffer();
    for row in 0..buffer.area.height {
        for column in 0..buffer.area.width {
            let suffix: String = (column..buffer.area.width)
                .map(|x| buffer[(x, row)].symbol())
                .collect();
            if suffix.starts_with(text) {
                return buffer[(column, row)].style();
            }
        }
    }
    panic!(
        "missing source text {text:?}:\n{}",
        crate::headless::buffer_text(buffer)
    );
}

#[test]
fn diff_syntax_survives_origin_tints_emphasis_and_palette_changes() {
    let request = request();
    let mut detail = detail(&request);
    detail.files[0].path = "main.rs".into();
    detail.files[0].patch =
        Some("@@ -1,3 +1,3 @@\n fn main() {\n-    let old = 1;\n+    let new = 2;\n }\n".into());
    let theme = Theme::catppuccin_mocha();
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail, &theme);
    view.open_delta();
    let keyword = style_at(&mut view, &theme, "let new");
    assert_eq!(keyword.fg, Some(theme.syntax.keyword));
    assert_eq!(keyword.bg, Some(theme.semantic.diff_add_bg));
    assert_eq!(
        style_at(&mut view, &theme, "new =").bg,
        Some(theme.semantic.diff_add_strong)
    );
    assert_eq!(
        style_at(&mut view, &theme, "old =").bg,
        Some(theme.semantic.diff_del_strong)
    );
    let dracula = Theme::embedded("dracula").unwrap();
    view.set_theme(&dracula);
    assert_eq!(
        style_at(&mut view, &dracula, "let new").fg,
        Some(dracula.syntax.keyword)
    );
}

#[test]
fn old_filename_and_hunk_gaps_define_independent_syntax_contexts() {
    let request = request();
    let mut detail = detail(&request);
    detail.files[0].path = "after.rs".into();
    detail.files[0].previous_path = Some("before.py".into());
    detail.files[0].patch =
        Some("@@ -1,2 +1 @@\n-def previous():\n-    pass\n+fn current() {}\n".into());
    let theme = Theme::catppuccin_mocha();
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail.clone(), &theme);
    view.open_delta();
    assert_eq!(
        style_at(&mut view, &theme, "def previous").fg,
        Some(theme.syntax.keyword)
    );
    assert_eq!(
        style_at(&mut view, &theme, "fn current").fg,
        Some(theme.syntax.keyword)
    );

    detail.files[0].previous_path = None;
    detail.files[0].patch = Some("@@ -1 +1 @@\n-/* omitted continuation\n+let first = 1;\n@@ -20 +20 @@\n-let before = 2;\n+let after = 3;\n".into());
    view.loaded(&request, detail, &theme);
    view.open_delta();
    assert_eq!(
        style_at(&mut view, &theme, "let before").fg,
        Some(theme.syntax.keyword)
    );
}

#[test]
fn sidebar_focus_changes_diff_and_file_stepping_respects_filter() {
    let request = request();
    let mut detail = detail(&request);
    detail.files[0].path = "first.rs".into();
    detail.files[0].patch = Some("@@ -0,0 +1 @@\n+fn first() {}\n".into());
    let mut second = detail.files[0].clone();
    second.path = "second.rs".into();
    second.patch = Some("@@ -0,0 +1 @@\n+fn second() {}\n".into());
    detail.files.push(second);
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail, &Theme::catppuccin_mocha());
    view.open_delta();
    assert!(frame(&mut view, 140, 20).contains("files (2)"));
    view.focus_next();
    view.move_cursor(ListMovement::Next);
    assert!(frame(&mut view, 140, 20).contains("fn second()"));
    view.begin_filter();
    for character in "first".chars() {
        view.filter_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    view.filter_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    view.step_file(ListMovement::Next);
    let screen = frame(&mut view, 140, 20);
    assert!(screen.contains("fn first()"));
    assert!(!screen.contains("fn second()"));
    view.begin_filter();
    view.filter_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    assert!(frame(&mut view, 140, 20).contains("no matching files"));
    view.filter_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(frame(&mut view, 140, 20).contains("fn first()"));
}

#[test]
fn wrapped_message_tail_is_reachable_through_shared_preview() {
    let request = request();
    let mut detail = detail(&request);
    detail.message = format!("{}\nEND_OF_MESSAGE", "long paragraph ".repeat(200));
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail, &Theme::catppuccin_mocha());
    view.focus_next();
    frame(&mut view, 100, 12);
    view.move_cursor(ListMovement::Last);
    assert!(frame(&mut view, 100, 12).contains("END_OF_MESSAGE"));
}

fn press(view: &mut CommitView, code: KeyCode, modifiers: KeyModifiers) {
    let action = view.handle_key(KeyEvent::new(code, modifiers));
    view.update(&action);
}

#[test]
fn diff_find_is_reversible_and_keeps_its_source_cursor() {
    let request = request();
    let mut detail = detail(&request);
    detail.files[0].path = "main.rs".into();
    detail.files[0].patch = Some(
        "@@ -0,0 +1,3 @@\n+let first_needle = 1;\n+let second_needle = 2;\n+let other = 3;\n"
            .into(),
    );
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail, &Theme::catppuccin_mocha());
    view.open_delta();
    frame(&mut view, 140, 20);
    for character in "/needle".chars() {
        press(&mut view, KeyCode::Char(character), KeyModifiers::NONE);
    }
    assert!(view.searching());
    assert!(view.copy_text().unwrap().contains("first_needle"));
    press(&mut view, KeyCode::Enter, KeyModifiers::NONE);
    press(&mut view, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(view.copy_text().unwrap().contains("second_needle"));
    for character in "/absent".chars() {
        press(&mut view, KeyCode::Char(character), KeyModifiers::NONE);
    }
    assert!(frame(&mut view, 140, 20).contains("0/0"));
    press(&mut view, KeyCode::Esc, KeyModifiers::NONE);
    assert!(view.copy_text().unwrap().contains("second_needle"));
    press(&mut view, KeyCode::Char('N'), KeyModifiers::NONE);
    assert!(view.copy_text().unwrap().contains("first_needle"));
}

#[test]
fn deleted_and_renamed_line_links_use_parent_identity() {
    let request = request();
    let mut detail = detail(&request);
    detail.parents = vec!["first-parent".into(), "other-parent".into()];
    detail.files[0].path = "src/new.rs".into();
    detail.files[0].previous_path = Some("old/name.rs".into());
    detail.files[0].patch = Some("@@ -10 +20 @@\n-old content\n+new content\n".into());
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail, &Theme::catppuccin_mocha());
    assert!(matches!(
        view.yank_target(),
        Ok(CommitYankTarget::Commit("https://example.test/commit"))
    ));
    view.open_delta();
    view.move_cursor(ListMovement::Next);
    match view.yank_target().unwrap() {
        CommitYankTarget::FileLine {
            path,
            revision,
            line,
            ..
        } => {
            assert_eq!(path.as_str(), "old/name.rs");
            assert_eq!(revision.as_str(), "first-parent");
            assert_eq!(line.get(), 10);
        }
        _ => panic!("deleted source row needs a source link"),
    }
    view.move_cursor(ListMovement::Next);
    match view.yank_target().unwrap() {
        CommitYankTarget::FileLine {
            path,
            revision,
            line,
            ..
        } => {
            assert_eq!(path.as_str(), "src/new.rs");
            assert_eq!(revision, &request.revision);
            assert_eq!(line.get(), 20);
        }
        _ => panic!("added source row needs a source link"),
    }
    assert_eq!(view.copy_text().unwrap(), "new content\n");
    view.begin_filter();
    for character in "old/name".chars() {
        view.filter_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    assert!(!frame(&mut view, 140, 20).contains("no matching files"));
}

#[test]
fn page_keys_move_source_cursor_by_viewport_not_one_line() {
    let request = request();
    let mut detail = detail(&request);
    let source: String = (0..100)
        .map(|index| format!("+// line {index}\n"))
        .collect();
    detail.files[0].patch = Some(format!("@@ -0,0 +1,100 @@\n{source}"));
    let mut view = CommitView::open(request.clone());
    view.loaded(&request, detail, &Theme::catppuccin_mocha());
    view.open_delta();
    view.move_cursor(ListMovement::Next);
    frame(&mut view, 140, 20);
    let before = view.copy_text().unwrap();
    press(&mut view, KeyCode::Char('f'), KeyModifiers::CONTROL);
    let after = view.copy_text().unwrap();
    assert_ne!(after, before);
    assert_ne!(after, "// line 1\n");
    press(&mut view, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(view.copy_text().unwrap(), before);
}

#[test]
fn hierarchy_projection_keeps_only_matching_leaves_and_ancestors() {
    let request = request();
    let mut detail = detail(&request);
    let template = detail.files[0].clone();
    detail.files = [
        "root.rs",
        "src/z.rs",
        "src/deep/a.rs",
        "docs/readme.md",
        "src/a.rs",
    ]
    .into_iter()
    .map(|path| {
        let mut file = template.clone();
        file.path = path.into();
        file
    })
    .collect();
    let content = CommitContent::new(detail);
    let names: Vec<_> = content
        .tree
        .file_order()
        .iter()
        .map(|file| content.files[file.get()].label.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "docs/readme.md",
            "src/deep/a.rs",
            "src/a.rs",
            "src/z.rs",
            "root.rs"
        ]
    );
    let rows = content.tree.visible_rows(&[ItemIndex::new(1)]);
    assert!(matches!(&rows[..], [
        files::FileTreeRow::Directory { label, depth: 0 },
        files::FileTreeRow::File { file, depth: 1 },
    ] if label == "src" && file.get() == 1));
}
