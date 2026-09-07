use super::*;

#[test]
fn filter_commit_triggers_blob_load_of_selected_file() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // drill into repo root
    // Live-filter to Cargo.toml, commit with Enter.
    app.handle_key(key(KeyCode::Char('/')));
    for c in "cargo.toml".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("loading"),
        "file meta/loading preview should show while blob is pending"
    );
    app.handle_key(key(KeyCode::Enter)); // commit filter
    // Blob fetch was requested; inject the response.
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: "abc1234def5678".into(),
        name: "Cargo.toml".into(),
        bytes: b"[package]\nname = \"ratatui\"\n".to_vec(),
    });
    let rows = render(&mut app, 100, 30);
    assert!(
        rows.join("\n").contains("[package]"),
        "highlighted blob should render after filter commit"
    );
}

#[test]
fn launch_popup_only_when_state_has_no_repos() {
    // Fresh state → popup opens automatically.
    let mut fresh = test_app();
    let screen = render(&mut fresh, 100, 30).join("\n");
    // Offline double names itself; the title is forge-driven now.
    assert!(
        screen.contains("search offline"),
        "fresh launch should open the search popup"
    );

    // Returning user (repos OR orgs in state) → straight into the browser.
    let state = rootle::state::State {
        recent_repos: vec!["ratatui/ratatui".into()],
        ..Default::default()
    };
    let (tx, _rx) = rootle::event::channel();
    let mut app = App::with(state, tx);
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        !screen.contains("search github"),
        "launch with recents should skip the popup"
    );
    assert!(screen.contains("BROWSE"));

    let (tx, _rx) = rootle::event::channel();
    let mut orgs_only = App::with(
        rootle::state::State {
            recent_orgs: vec!["ratatui".into()],
            ..Default::default()
        },
        tx,
    );
    let screen = render(&mut orgs_only, 100, 30).join("\n");
    assert!(
        !screen.contains("search github"),
        "orgs-only history should also skip the popup"
    );
}

/// plans/0023 breaker F1: a failed blob fetch re-shows the honest
/// error on every re-select — never the "loading…" placeholder that
/// nothing will resolve — and explicit reload (␣ r) is the retry.
#[test]
fn failed_blob_restates_error_on_reselect() {
    fn file(path: &str, sha: &str) -> rootle_provider::TreeNode {
        rootle_provider::TreeNode {
            path: path.into(),
            is_dir: false,
            sha: sha.into(),
            size: Some(9),
        }
    }
    let mut app = app_with_orgs(&["ratatui"]);
    app.handle_key(key(KeyCode::Esc));
    app.handle_action(rootle::action::Action::OrgSelected("ratatui".into()));
    app.handle_action(rootle::action::Action::OrgReposLoaded {
        org: "ratatui".into(),
        repos: vec!["ratatui".into()],
    });
    app.handle_action(rootle::action::Action::TreeLoaded {
        owner: "ratatui".into(),
        name: "ratatui".into(),
        entries: vec![
            file("a.bin", "aaaaaaa1111111"),
            file("b.bin", "bbbbbbb2222222"),
        ],
        truncated: false,
        branch: "main".into(),
    });
    // a.bin selected; its fetch fails.
    app.handle_action(rootle::action::Action::BlobFailed {
        sha: "aaaaaaa1111111".into(),
        error: rootle_provider::ProviderError::other("binary file"),
    });
    let screen = render(&mut app, 80, 24).join("\n");
    assert!(
        screen.contains("error: binary file"),
        "first failure:\n{screen}"
    );

    // Move away and back: the error must persist, not regress to
    // "loading…" (the breaker's stuck-placeholder bug).
    app.handle_key(key(KeyCode::Char('j'))); // b.bin
    app.handle_key(key(KeyCode::Char('k'))); // a.bin again
    let screen = render(&mut app, 80, 24).join("\n");
    assert!(
        screen.contains("error: binary file"),
        "re-select:\n{screen}"
    );
    assert!(
        !screen.contains("loading…"),
        "no placeholder regression:\n{screen}"
    );

    // ␣ r clears the failure cache — the retry path re-requests and
    // the placeholder is honest again (a fetch really is in flight).
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('r')));
    let screen = render(&mut app, 80, 24).join("\n");
    assert!(screen.contains("loading…"), "reload retries:\n{screen}");
}

/// plans/0023 breaker F1b: a stale failure (user moved on while the
/// fetch was dying) must not clobber the visible preview.
#[test]
fn stale_blob_failure_does_not_clobber_visible_preview() {
    fn file(path: &str, sha: &str) -> rootle_provider::TreeNode {
        rootle_provider::TreeNode {
            path: path.into(),
            is_dir: false,
            sha: sha.into(),
            size: Some(9),
        }
    }
    let mut app = app_with_orgs(&["ratatui"]);
    app.handle_key(key(KeyCode::Esc));
    app.handle_action(rootle::action::Action::OrgSelected("ratatui".into()));
    app.handle_action(rootle::action::Action::OrgReposLoaded {
        org: "ratatui".into(),
        repos: vec!["ratatui".into()],
    });
    app.handle_action(rootle::action::Action::TreeLoaded {
        owner: "ratatui".into(),
        name: "ratatui".into(),
        entries: vec![
            file("a.bin", "aaaaaaa1111111"),
            file("b.bin", "bbbbbbb2222222"),
        ],
        truncated: false,
        branch: "main".into(),
    });
    app.handle_key(key(KeyCode::Char('j'))); // b.bin before a.bin's reply
    app.handle_action(rootle::action::Action::BlobFailed {
        sha: "aaaaaaa1111111".into(),
        error: rootle_provider::ProviderError::other("binary file"),
    });
    let screen = render(&mut app, 80, 24).join("\n");
    assert!(screen.contains("b.bin"), "visible file:\n{screen}");
    assert!(
        !screen.contains("error: binary file"),
        "stale failure clobbered the preview:\n{screen}"
    );
}

/// plans/0023 breaker F5: popups have a size floor — at 40×10 the
/// search popup renders a complete results box inside its border; at
/// 30×6 (below the floor) it renders the input alone rather than a
/// guillotined box leaking cells.
#[test]
fn search_popup_respects_size_floor() {
    let mut app = test_app(); // fresh state: launch popup open
    let rows = render(&mut app, 40, 10);
    let text = rows.join("\n");
    assert!(text.contains("search offline"), "popup open:\n{text}");
    // The results box closes (└) above the hint row, which rides the
    // popup's own bottom border.
    let hint_row = rows
        .iter()
        .position(|r| r.contains("tab focus"))
        .expect("hint row present");
    let closes_above = rows[..hint_row].iter().any(|r| r.contains('└'));
    assert!(closes_above, "results box closes inside the popup:\n{text}");

    let mut app = test_app();
    let rows = render(&mut app, 30, 6);
    let text = rows.join("\n");
    assert!(
        !text.contains("results"),
        "below the floor: no results box at all:\n{text}"
    );
    assert!(text.contains("❯"), "input still renders:\n{text}");
}

/// plans/0023 breaker round 3: a background success must never erase
/// a fresh error from an unrelated in-flight operation — the direct-
/// arg repo's 404 was wiped by the default-org warm-up landing after.
#[test]
fn background_success_never_erases_a_fresh_error() {
    let mut app = browsing_app();
    // The failing fetch lands first (direct-arg 404)…
    app.handle_action(rootle::action::Action::TreeFailed {
        owner: "zzz".into(),
        name: "nope".into(),
        error: rootle_provider::ProviderError::other("HTTP 404 Not Found"),
    });
    assert!(
        app.snapshot()["status"]
            .as_str()
            .unwrap()
            .contains("HTTP 404"),
        "error visible"
    );
    // …then an unrelated org-repos success arrives: the error stays.
    app.handle_app_event(rootle::event::AppEvent::OrgReposLoaded {
        org: "ratatui".into(),
        repos: vec![],
    });
    assert!(
        app.snapshot()["status"]
            .as_str()
            .unwrap()
            .contains("HTTP 404"),
        "success must not erase the error"
    );
    // Its own loading marker, though, clears on success.
    app.handle_action(rootle::action::Action::LoadOrgRepos("tokio-rs".into()));
    assert_eq!(app.snapshot()["status"], "loading tokio-rs…");
    app.handle_app_event(rootle::event::AppEvent::OrgReposLoaded {
        org: "tokio-rs".into(),
        repos: vec![],
    });
    assert_eq!(app.snapshot()["status"], serde_json::Value::Null);
}
