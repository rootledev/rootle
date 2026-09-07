use super::*;

/// 0019 polish: a file's last commit dresses the preview header band
/// (GitHub's file header), the memo dresses re-selects instantly.
#[test]
fn last_commit_band_dresses_the_preview_header() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // drill into repo root
    app.handle_key(key(KeyCode::Char('/')));
    for c in "cargo.toml".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter)); // commit the filter
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: "abc1234def5678".into(),
        name: "Cargo.toml".into(),
        bytes: b"[package]\nname = \"ratatui\"\n".to_vec(),
    });
    // No band context until the ambient fetch lands.
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(!screen.contains("shader cache"), "no commit yet:\n{screen}");

    app.handle_app_event(rootle::event::AppEvent::LastCommitLoaded {
        repo: "ratatui/ratatui".into(),
        path: "Cargo.toml".into(),
        entry: Some(rootle_provider::LogEntry {
            sha: "d00d1234feed".into(),
            subject: "shader cache: memoize passes".into(),
            author: "Stu".into(),
            date: "2026-08-21".into(),
        }),
    });
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(
        screen.contains("d00d123 · shader cache"),
        "band sha+subject:\n{screen}"
    );
    assert!(screen.contains("Stu"), "band author:\n{screen}");
}

/// plans/0016 M1a: `␣ b` opens the revision switcher (loading row
/// until refs land), the crumb live-previews the cursor's ref, Enter
/// commits and refetches, Esc reverts to the committed ref.
#[test]
fn refs_switcher_preview_commit_revert() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('b')));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("revisions"), "switcher missing:\n{screen}");
    assert!(
        screen.contains("loading revisions"),
        "loading row:\n{screen}"
    );

    app.handle_app_event(rootle::event::AppEvent::RefsLoaded {
        repo: "ratatui/ratatui".into(),
        refs: rootle_provider::RepoRefs {
            branches: vec![
                rootle_provider::RefInfo {
                    name: "main".into(),
                    sha: "a1b2c3d4".into(),
                    is_default: true,
                },
                rootle_provider::RefInfo {
                    name: "release/2.7".into(),
                    sha: "e4f5a6b7".into(),
                    is_default: false,
                },
            ],
            tags: vec![rootle_provider::RefInfo {
                name: "v0.7.1".into(),
                sha: "d4e5f6a7".into(),
                is_default: false,
            }],
        },
    });
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("(•) main"), "default radio:\n{screen}");
    assert!(
        screen.contains("release/2.7  e4f5a6b"),
        "branch row:\n{screen}"
    );
    assert!(screen.contains("tags"), "tags section:\n{screen}");

    // Live preview: j moves to release/2.7, the crumb follows.
    app.handle_key(key(KeyCode::Char('j')));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(
        screen.contains("@ release/2.7"),
        "crumb did not follow the cursor:\n{screen}"
    );

    // Enter commits: popup closes, the crumb keeps the ref.
    app.handle_key(key(KeyCode::Enter));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(!screen.contains("revisions —"), "popup should be closed");
    assert!(
        screen.contains("ratatui @ release/2.7"),
        "committed crumb:\n{screen}"
    );

    // Reopen and Esc: reverts to the committed ref.
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('b')));
    app.handle_app_event(rootle::event::AppEvent::RefsLoaded {
        repo: "ratatui/ratatui".into(),
        refs: rootle_provider::RepoRefs {
            branches: vec![rootle_provider::RefInfo {
                name: "main".into(),
                sha: "a1b2c3d4".into(),
                is_default: true,
            }],
            tags: vec![],
        },
    });
    app.handle_key(key(KeyCode::Char('k'))); // preview main
    app.handle_key(key(KeyCode::Esc));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(
        screen.contains("@ release/2.7"),
        "Esc must revert:\n{screen}"
    );
}

#[test]
fn preview_submode_zoom_blame_history() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // into ratatui root
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j'))); // onto Cargo.toml
    }
    let sha = "abc1234def5678";
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: sha.into(),
        name: "Cargo.toml".into(),
        bytes: b"[package]\nname = \"ratatui\"\nversion = \"0.29\"\nauthors = []\n".to_vec(),
    });
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("[package]"), "preview shows the blob");

    // ␣ p: zoom — the columns vanish, the pane owns the keys.
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('p')));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(
        screen.contains("PREVIEW"),
        "preview chip missing:\n{screen}"
    );
    assert!(!screen.contains("docs/"), "columns must hide when zoomed");

    // b: blame fetches; marks render when ranges land.
    app.handle_key(key(KeyCode::Char('b')));
    app.handle_app_event(rootle::event::AppEvent::BlameLoaded {
        path: "Cargo.toml".into(),
        ranges: vec![
            rootle_provider::BlameRange {
                start_line: 1,
                end_line: 2,
                sha: "a1b2c3d4e5".into(),
                author: "tarek".into(),
                date: "2026-08-25".into(),
            },
            rootle_provider::BlameRange {
                start_line: 3,
                end_line: 4,
                sha: "c7d8e9f0a1".into(),
                author: "mira".into(),
                date: "2026-07-30".into(),
            },
        ],
    });
    let rows = render(&mut app, 200, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("a1b2c3d tarek"), "blame margin:\n{screen}");
    assert!(screen.contains("c7d8e9f mira"), "second run:\n{screen}");
    // The margin divider aligns across run starts and continuations.
    let divider_col = |row: &String| {
        row.chars()
            .enumerate()
            .filter(|(_, c)| *c == '│')
            .nth(1)
            .map(|(i, _)| i)
    };
    let run = rows.iter().find(|r| r.contains("a1b2c3d tarek")).unwrap();
    let cont = rows.iter().find(|r| r.contains('·')).unwrap();
    assert_eq!(
        divider_col(run),
        divider_col(cont),
        "blame divider misaligned:\n{screen}"
    );

    // Enter on a blame line: history lens at that commit (composition).
    app.handle_key(key(KeyCode::Enter));
    app.handle_app_event(rootle::event::AppEvent::LogLoaded {
        path: "Cargo.toml".into(),
        entries: vec![
            rootle_provider::LogEntry {
                sha: "c7d8e9f0a1b2".into(),
                subject: "refactor(Cargo.toml): extract".into(),
                author: "mira".into(),
                date: "2026-07-30".into(),
            },
            rootle_provider::LogEntry {
                sha: "a1b2c3d4e5f6".into(),
                subject: "feat: initial Cargo.toml".into(),
                author: "tarek".into(),
                date: "2026-06-11".into(),
            },
        ],
        truncated: false,
    });
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("HISTORY"), "history chip:\n{screen}");
    assert!(
        screen.contains("history — Cargo.toml"),
        "lens title:\n{screen}"
    );
    // The cursor line (1) blames to the initial commit — the lens
    // sits on its row.
    assert!(
        screen.contains("▌ a1b2c3d feat: initial Cargo.toml"),
        "lens must sit on the blamed commit:\n{screen}"
    );

    // Enter opens the file at the commit; Esc restores the present.
    app.handle_key(key(KeyCode::Enter));
    app.handle_app_event(rootle::event::AppEvent::BlobAtLoaded {
        path: "Cargo.toml".into(),
        ref_: "c7d8e9f0a1b2".into(),
        sha: "f00dbabe".into(),
        bytes: b"[package]\nname = \"ratatui-old\"\n".to_vec(),
        subject: "refactor(Cargo.toml): extract".into(),
        author: "mira".into(),
        date: "2026-07-30".into(),
    });
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("ratatui-old"), "commit content:\n{screen}");
    // The header band: full path left, commit context right.
    assert!(
        screen.contains("refactor(Cargo.toml): extract · mira · 2026-07-30"),
        "band context missing:\n{screen}"
    );
    assert!(
        screen.contains("Cargo.toml @ c7d8e9f"),
        "commit marker:\n{screen}"
    );
    assert!(screen.contains("PREVIEW"), "back in the zoomed pane");
    // The regression from the demo: at-commit content highlights by
    // the real path — the "@ sha" marker has no known extension.
    // (Language evidence lives in e2e/test_revisions.py, where the
    // fixture file is real rust; syntect's fancy set has no TOML, so
    // this frame's footer honestly reads "text".)
    app.handle_key(key(KeyCode::Esc));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(
        screen.contains("name = \"ratatui\""),
        "Esc restores the present"
    );
    assert!(
        !screen.contains("Cargo.toml @"),
        "the marker must not stick after restore:\n{screen}"
    );
    app.handle_key(key(KeyCode::Esc));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(screen.contains("BROWSE"), "mode unwinds");
    assert!(screen.contains("docs/"), "columns restored");
}

/// plans/0023 breaker F3: the "blame…" transient clears when the
/// blame lands — and only ever its own, never a newer status.
#[test]
fn blame_loaded_clears_only_its_own_transient() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // into ratatui root
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j'))); // onto Cargo.toml
    }
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: "abc1234def5678".into(),
        name: "Cargo.toml".into(),
        bytes: b"[package]\nname = \"ratatui\"\n".to_vec(),
    });
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('p')));
    app.handle_key(key(KeyCode::Char('b')));
    assert_eq!(app.snapshot()["status"], "blame…", "loading status set");

    // The happy path: blame lands, its transient is gone.
    let mut fresh = app;
    fresh.handle_app_event(rootle::event::AppEvent::BlameLoaded {
        path: "Cargo.toml".into(),
        ranges: vec![],
    });
    assert_eq!(fresh.snapshot()["status"], serde_json::Value::Null);

    // The stale-reply path: a newer status (the failure) must survive
    // an out-of-order BlameLoaded.
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l')));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: "abc1234def5678".into(),
        name: "Cargo.toml".into(),
        bytes: b"[package]\nname = \"ratatui\"\n".to_vec(),
    });
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('p')));
    app.handle_key(key(KeyCode::Char('b')));
    app.handle_app_event(rootle::event::AppEvent::BlameFailed {
        path: "Cargo.toml".into(),
        error: rootle_provider::ProviderError::other("boom"),
    });
    app.handle_app_event(rootle::event::AppEvent::BlameLoaded {
        path: "Cargo.toml".into(),
        ranges: vec![],
    });
    assert_eq!(app.snapshot()["status"], "boom", "newer status survives");
}
