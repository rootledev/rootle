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
        request: app.owner_request().unwrap().clone(),
        repos: vec!["ratatui".into()],
    });
    app.handle_key(key(KeyCode::Char('l')));
    app.handle_action(rootle::action::Action::TreeLoaded {
        request: app.tree_request().unwrap().clone(),
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
        request: app.owner_request().unwrap().clone(),
        repos: vec!["ratatui".into()],
    });
    app.handle_key(key(KeyCode::Char('l')));
    app.handle_action(rootle::action::Action::TreeLoaded {
        request: app.tree_request().unwrap().clone(),
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

#[test]
fn owner_failure_cannot_replace_a_ready_personal_repository() {
    use rootle::action::Action;
    use rootle::event::AppEvent;
    for owner_finishes_first in [true, false] {
        let mut app = app_with_orgs(&["personal"]);
        app.handle_action(Action::LoadOrgRepos("personal".into()));
        let owner = app.owner_request().unwrap().clone();
        app.handle_action(Action::RepoSelected {
            owner: "personal".into(),
            name: "project".into(),
        });
        let tree = app.tree_request().unwrap().clone();
        let failure = AppEvent::OrgReposFailed {
            request: owner,
            error: rootle_provider::ProviderError::new(
                rootle_provider::ErrorKind::NotFound,
                "owner listing unavailable",
            ),
        };
        if owner_finishes_first {
            app.handle_app_event(failure);
            app.handle_app_event(AppEvent::TreeLoaded {
                request: tree,
                entries: ratatui_tree(),
                truncated: false,
                branch: "main".into(),
            });
        } else {
            app.handle_app_event(AppEvent::TreeLoaded {
                request: tree,
                entries: ratatui_tree(),
                truncated: false,
                branch: "main".into(),
            });
            app.handle_app_event(failure);
        }
        let state = app.snapshot();
        assert_eq!(state["browser"]["tree"]["phase"], "ready");
        assert_eq!(
            state["browser"]["tree"]["entry_count"],
            ratatui_tree().len()
        );
        assert_eq!(state["browser"]["owner_list"]["error"]["kind"], "not_found");
        assert_eq!(state["browser"]["owner_kind"], "unknown");
        assert!(state["status"].is_null());
        assert!(render(&mut app, 100, 30).join("\n").contains("Cargo.toml"));
    }
}

#[test]
fn owner_success_cannot_replace_a_selected_tree() {
    use rootle::action::Action;
    let mut app = app_with_orgs(&["personal"]);
    app.handle_action(Action::LoadOrgRepos("personal".into()));
    let owner = app.owner_request().unwrap().clone();
    app.handle_action(Action::RepoSelected {
        owner: "personal".into(),
        name: "project".into(),
    });
    app.handle_action(Action::TreeLoaded {
        request: app.tree_request().unwrap().clone(),
        entries: ratatui_tree(),
        truncated: true,
        branch: "main".into(),
    });
    // Returning to the owner column does not revive an older request's navigation intent.
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_key(key(KeyCode::Char('h')));
    let before = app.snapshot()["browser"]["pane"].clone();
    app.handle_action(Action::OrgReposLoaded {
        request: owner,
        repos: vec!["different".into()],
    });
    let after = app.snapshot();
    assert_eq!(after["browser"]["pane"], before);
    assert_eq!(
        after["browser"]["tree"]["request"]["repository"],
        "personal/project"
    );
    assert_eq!(after["browser"]["tree"]["truncated"], true);
}

#[test]
fn stale_tree_results_do_not_mutate_current_request_and_empty_is_ready() {
    use rootle::action::Action;
    let mut app = browsing_app();
    app.handle_action(Action::LeaderReload);
    let old = app.tree_request().unwrap().clone();
    app.handle_action(Action::LeaderReload);
    let current = app.tree_request().unwrap().clone();
    let before = app.snapshot();
    app.handle_action(Action::TreeFailed {
        request: old.clone(),
        error: "obsolete failure".into(),
    });
    app.handle_action(Action::TreeLoaded {
        request: old,
        entries: vec![],
        truncated: true,
        branch: "obsolete".into(),
    });
    assert_eq!(app.snapshot(), before);
    app.handle_action(Action::TreeLoaded {
        request: current,
        entries: vec![],
        truncated: false,
        branch: "main".into(),
    });
    let state = app.snapshot();
    assert_eq!(state["browser"]["tree"]["phase"], "ready");
    assert_eq!(state["browser"]["tree"]["entry_count"], 0);
    assert_eq!(state["browser"]["tree"]["truncated"], false);
}

#[test]
fn failed_revision_reload_distinguishes_retained_tree_from_current_result() {
    use rootle::action::Action;
    let mut app = browsing_app();
    let previous = app.tree_request().unwrap().clone();
    app.handle_action(Action::RefsCommit("topic".into()));
    let current = app.tree_request().unwrap().clone();
    let before = app.snapshot();
    app.handle_action(Action::TreeLoaded {
        request: previous,
        entries: vec![],
        truncated: false,
        branch: "wrong".into(),
    });
    assert_eq!(app.snapshot(), before);
    app.handle_action(Action::TreeFailed {
        request: current,
        error: "topic unavailable".into(),
    });
    let state = app.snapshot();
    assert_eq!(state["browser"]["tree"]["phase"], "failed");
    assert!(state["browser"]["tree"]["entry_count"].is_null());
    assert_eq!(state["browser"]["tree"]["request"]["revision"], "topic");
    assert_eq!(state["browser"]["tree"]["displayed"]["stale"], true);
    assert!(state["browser"]["tree"]["displayed"]["request"]["revision"].is_null());
}

#[test]
fn moving_to_another_repository_rejects_pending_tree_before_a_new_load() {
    use rootle::action::Action;
    let mut app = browsing_app();
    app.handle_action(Action::LeaderReload);
    let obsolete = app.tree_request().unwrap().clone();
    app.handle_key(key(KeyCode::Char('j')));
    let moved = app.snapshot();
    assert_eq!(moved["browser"]["tree"]["phase"], "idle");
    assert!(moved["browser"]["tree"]["displayed"].is_null());
    app.handle_action(Action::TreeLoaded {
        request: obsolete.clone(),
        entries: ratatui_tree(),
        truncated: false,
        branch: "obsolete".into(),
    });
    app.handle_action(Action::TreeFailed {
        request: obsolete,
        error: "obsolete failure".into(),
    });
    assert_eq!(app.snapshot(), moved);
}
