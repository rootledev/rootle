use super::*;

/// plans/0028 — the commit viewer: history `d` dives into commit
/// detail, Enter opens the file delta, `]f` steps files, the Esc
/// ladder unwinds delta → detail → history.
#[test]
fn commit_viewer_dive_chain() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // into ratatui root
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j'))); // onto Cargo.toml
    }
    let sha = "abc1234def5678";
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: sha.into(),
        name: "Cargo.toml".into(),
        bytes: b"[package]\nname = \"ratatui\"\n".to_vec(),
    });

    // ␣ p h: the history lens over the previewed file.
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('p')));
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_app_event(rootle::event::AppEvent::LogLoaded {
        request: app.active_history_request().unwrap().clone(),
        entries: vec![rootle_provider::LogEntry {
            sha: "feedface1234".into(),
            subject: "feat: wire the widget".into(),
            author: "tarek".into(),
            date: "2026-09-06".into(),
        }],
        truncated: false,
    });

    // d: dive into the commit.
    app.handle_key(key(KeyCode::Char('d')));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("loading commit…"), "loading row:\n{screen}");
    app.handle_app_event(rootle::event::AppEvent::CommitLoaded {
        request: app.active_commit_request().unwrap().clone(),
        detail: Ok(rootle_provider::CommitDetail {
            sha: "feedface1234".into(),
            author: "tarek".into(),
            date: "2026-09-06".into(),
            message: "feat: wire the widget\n\nTwo files touched.\n".into(),
            parents: vec!["abc1234def5678".into()],
            truncated: false,
            web_url: Some("https://github.com/ratatui/ratatui/commit/feedface1234".into()),
            files: vec![
                rootle_provider::CommitFile {
                    path: "src/lib.rs".into(),
                    status: rootle_provider::FileStatus::Modified,
                    additions: Some(2),
                    deletions: Some(1),
                    patch: Some(
                        "@@ -10,3 +10,4 @@\n context line\n-old line\n+new line one\n+new line two\n context tail\n".into(),
                    ),
                    previous_path: None,
                    binary: false,
                },
                rootle_provider::CommitFile {
                    path: "README.md".into(),
                    status: rootle_provider::FileStatus::Added,
                    additions: Some(5),
                    deletions: None,
                    patch: None, // no hunks shown until opened; added files still parse
                    previous_path: None,
                    binary: false,
                },
            ],
        }),
    });
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("COMMIT"), "commit chip:\n{screen}");
    assert!(
        screen.contains("feedfac") && screen.contains("tarek"),
        "header band carries sha + author:\n{screen}"
    );
    assert!(
        screen.contains("src/lib.rs") && screen.contains("README.md"),
        "changed files listed:\n{screen}"
    );
    assert!(
        screen.contains("Two files touched."),
        "message body:\n{screen}"
    );

    // Enter: the first file's delta.
    app.handle_key(key(KeyCode::Enter));
    let rows = render(&mut app, 200, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("@@ -10,3 +10,4 @@"),
        "hunk header:\n{screen}"
    );
    assert!(screen.contains("new line one"), "added line:\n{screen}");
    assert!(screen.contains("old line"), "deleted line:\n{screen}");
    assert!(screen.contains("1/2"), "file position in title:\n{screen}");

    // Style proof: the sign column carries the origin colors (Buffer
    // cells, not text) — quiet tints come from theme roles.
    {
        let backend = TestBackend::new(200, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                app.render(f, area);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let saw = |hex: u32| {
            let want = ratatui::style::Color::from_u32(hex);
            buf.content
                .iter()
                .any(|c| c.symbol() == "▎" && c.fg == want)
        };
        assert!(saw(0xa9c47c), "added-line sign in diff_add_fg");
        assert!(saw(0xe8677a), "deleted-line sign in diff_del_fg");
    }

    // ]f: step to the next file (added README — 5 additions).
    app.handle_key(key(KeyCode::Char(']')));
    app.handle_key(key(KeyCode::Char('f')));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("2/2"), "stepped to file two:\n{screen}");

    // The Esc ladder: delta closes to the detail list…
    app.handle_key(key(KeyCode::Esc));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(
        screen.contains("files (2)"),
        "back on the detail list:\n{screen}"
    );
    // …the next Esc closes the viewer to the history lens…
    app.handle_key(key(KeyCode::Esc));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("HISTORY"), "unwound to history:\n{screen}");
    assert!(
        screen.contains("history — Cargo.toml"),
        "history lens intact under the viewer:\n{screen}"
    );
}

#[test]
fn repository_history_rejects_stale_successes_and_failures_after_reopening() {
    let mut app = browsing_app();
    app.handle_action(rootle::action::Action::LeaderRepositoryHistory);
    let stale = app.active_history_request().unwrap().clone();
    app.handle_action(rootle::action::Action::HistoryClose);
    app.handle_action(rootle::action::Action::LeaderRepositoryHistory);
    let current = app.active_history_request().unwrap().clone();
    let status = app.snapshot()["status"].clone();
    app.handle_app_event(rootle::event::AppEvent::LogLoaded {
        request: stale.clone(),
        entries: vec![rootle_provider::LogEntry {
            sha: "obsolete".into(),
            subject: "obsolete commit".into(),
            author: "old".into(),
            date: "old".into(),
        }],
        truncated: false,
    });
    app.handle_app_event(rootle::event::AppEvent::LogFailed {
        request: stale,
        error: rootle_provider::ProviderError::new(
            rootle_provider::ErrorKind::Provider,
            "obsolete error",
        ),
    });
    assert_eq!(app.snapshot()["status"], status);
    let screen = render(&mut app, 120, 20).join("\n");
    assert!(screen.contains("loading history"));
    assert!(!screen.contains("obsolete"));
    app.handle_app_event(rootle::event::AppEvent::LogFailed {
        request: current,
        error: rootle_provider::ProviderError::new(
            rootle_provider::ErrorKind::Provider,
            "current history failed",
        ),
    });
    let screen = render(&mut app, 120, 20).join("\n");
    assert!(screen.contains("current history failed"));
    assert!(!screen.contains("loading history"));
}
