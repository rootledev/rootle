use super::*;

#[test]
fn hit_expand_shows_full_file_at_anchor_and_esc_restores_results() {
    let mut hit = rootle::components::global_search::SearchHit::plain(
        "owner/repo",
        "src/main.rs",
        6,
        vec![(6, "let view = render();".to_string())],
        1,
        String::new(),
    );
    hit.sha = "blob123".into();
    let mut app = grep_view_on_hit(hit);

    // Enter expands: the pane opens as a loading placeholder while
    // the blob is on its way.
    app.handle_key(key(KeyCode::Enter));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("owner/repo/src/main.rs:6"),
        "file pane title names repo/path:line:\n{screen}"
    );
    assert!(screen.contains("loading"), "fetch in flight:\n{screen}");

    // The blob lands (UI-thread styled): the WHOLE file renders with
    // the cursor on the anchor line.
    let lines: Vec<_> = (1..=40)
        .map(|i| ratatui::text::Line::from(format!("src line {i} of the file")))
        .collect();
    app.handle_action(rootle::action::Action::HitFileLoaded {
        repo: "owner/repo".into(),
        path: "src/main.rs".into(),
        sha: "blob123".into(),
        lang: "rust".into(),
        lines,
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("src line 18 of the file"),
        "content past the folded preview's reach renders:\n{screen}"
    );
    assert!(
        screen.contains('┃'),
        "the 40-line file overflows the pane — scrollbar proves it:\n{screen}"
    );
    assert!(
        screen.contains("6/40"),
        "readout puts the cursor on the anchor:\n{screen}"
    );
    assert!(
        screen.contains("rust · 40 lines"),
        "footer carries the language:\n{screen}"
    );

    // j walks the file cursor; the readout follows.
    app.handle_key(key(KeyCode::Char('j')));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("7/40"),
        "j moves the line cursor:\n{screen}"
    );

    // Esc folds back: the results list returns, selection intact, and
    // no file content lingers anywhere on the screen.
    app.handle_key(key(KeyCode::Esc));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("src/main.rs"),
        "results list back with the hit selected:\n{screen}"
    );
    assert!(
        screen.contains("let view = render();"),
        "the hit's folded preview renders again:\n{screen}"
    );
    assert!(
        !screen.contains("of the file"),
        "no lingering file-pane cells:\n{screen}"
    );
    assert!(!screen.contains("6/40"), "readout gone with the pane");
}

/// 0019 parity: `b` in the expanded pane runs the blame lens there —
/// the fetch lands, the run margins render in the pane.
#[test]
fn expanded_pane_blame_lens_renders() {
    let mut hit = rootle::components::global_search::SearchHit::plain(
        "owner/repo",
        "src/main.rs",
        2,
        vec![(2, "let x = 1;".to_string())],
        1,
        String::new(),
    );
    hit.sha = "blob123".into();
    hit.branch = "main".into();
    let mut app = grep_view_on_hit(hit);
    app.handle_key(key(KeyCode::Enter)); // expand
    let lines: Vec<_> = (1..=30)
        .map(|i| ratatui::text::Line::from(format!("src line {i}")))
        .collect();
    app.handle_action(rootle::action::Action::HitFileLoaded {
        repo: "owner/repo".into(),
        path: "src/main.rs".into(),
        sha: "blob123".into(),
        lang: "rust".into(),
        lines,
    });

    // `b` opens the lens (fetch in flight), the ranges land, margins
    // render with the run sha + author.
    app.handle_key(key(KeyCode::Char('b')));
    app.handle_app_event(rootle::event::AppEvent::BlameLoaded {
        path: "src/main.rs".into(),
        ranges: vec![rootle_provider::BlameRange {
            start_line: 1,
            end_line: 30,
            sha: "abcdef1234".into(),
            author: "Tarek".into(),
            date: "2026-08-28".into(),
        }],
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("abcdef1") || screen.contains("Tarek"),
        "blame run margins render in the pane:\n{screen}"
    );

    // `b` again drops the lens.
    app.handle_key(key(KeyCode::Char('b')));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        !screen.contains("abcdef1"),
        "second b clears the lens:\n{screen}"
    );
}

#[test]
fn file_pane_find_in_file_reuses_preview_session() {
    let mut hit = rootle::components::global_search::SearchHit::plain(
        "owner/repo",
        "src/main.rs",
        2,
        vec![(2, "let view = render();".to_string())],
        1,
        String::new(),
    );
    hit.sha = "blob123".into();
    let mut app = grep_view_on_hit(hit);
    app.handle_key(key(KeyCode::Enter));
    let lines: Vec<_> = (1..=6)
        .map(|i| {
            ratatui::text::Line::from(if i % 2 == 0 {
                format!("call render() {i}")
            } else {
                format!("plain line {i}")
            })
        })
        .collect();
    app.handle_action(rootle::action::Action::HitFileLoaded {
        repo: "owner/repo".into(),
        path: "src/main.rs".into(),
        sha: "blob123".into(),
        lang: "rust".into(),
        lines,
    });

    // `/` opens FIND over the file — the modeline chip flips and the
    // query rides the pane title, exactly like the browser's `␣ /`.
    app.handle_key(key(KeyCode::Char('/')));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("FIND"),
        "FIND chip over the pane:\n{screen}"
    );
    for c in "render".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("src/main.rs:2 /render"),
        "find query rides the pane title:\n{screen}"
    );
    assert!(
        screen.contains("1/3 · 2/6"),
        "match-of-matches readout:\n{screen}"
    );
    // Enter commits the chips; Esc folds the pane back to the results.
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Esc));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("results"),
        "Esc returns to the results list:\n{screen}"
    );
    assert!(
        !screen.contains("/render"),
        "the pane and its find session are gone:\n{screen}"
    );
}

#[test]
fn path_only_hit_expands_to_top_of_file() {
    // match_count 0, no anchor line (file-find shape): the file still
    // opens — cursor at the top, no `:0` in the title.
    let mut hit = rootle::components::global_search::SearchHit::plain(
        "owner/repo",
        "docs/readme.md",
        0,
        vec![],
        0,
        String::new(),
    );
    hit.sha = "blob456".into();
    let mut app = grep_view_on_hit(hit);
    app.handle_key(key(KeyCode::Enter));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("owner/repo/docs/readme.md "),
        "title omits the :0 anchor:\n{screen}"
    );
    app.handle_action(rootle::action::Action::HitFileLoaded {
        repo: "owner/repo".into(),
        path: "docs/readme.md".into(),
        sha: "blob456".into(),
        lang: "markdown".into(),
        lines: (1..=4)
            .map(|i| ratatui::text::Line::from(format!("doc line {i}")))
            .collect(),
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("1/4"),
        "unknown anchor falls back to the top:\n{screen}"
    );
    // Collapse still works from the path-only pane.
    app.handle_key(key(KeyCode::Char('h')));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(!screen.contains("doc line"), "h folds the pane back");
}

#[test]
fn mock_hit_expands_from_its_body_without_a_fetch() {
    // Offline submit → the mock producer's hits carry bodies; Enter
    // renders the file straight away (no loading state, no worker).
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter)); // expand the selected mock hit
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("mock content for src/widgets/list.rs"),
        "body renders without a fetch:\n{screen}"
    );
    assert!(
        !screen.contains("loading"),
        "body hits never show the placeholder:\n{screen}"
    );
}
