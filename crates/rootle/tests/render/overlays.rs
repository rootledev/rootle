use super::*;

#[test]
fn popup_results_support_local_slash_filter() {
    let mut app = test_app();
    // Submit a query (offline: no worker spawned) and inject the
    // response — what the worker thread would send over the channel.
    app.handle_key(key(KeyCode::Enter));
    app.handle_action(rootle::action::Action::SearchResults {
        items: vec![
            rootle_provider::SearchItem::Org("tokio-rs".into()),
            rootle_provider::SearchItem::Repo("tokio-rs/tokio".into()),
            rootle_provider::SearchItem::Repo("ratatui/ratatui".into()),
            rootle_provider::SearchItem::Repo("sharkdp/bat".into()),
        ],
    });
    assert!(
        app.cursor_style().is_none(),
        "results focus: no text cursor"
    );

    // `/` in results → SEARCH chip, incremental local filter.
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Char('t')));
    app.handle_key(key(KeyCode::Char('o')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("SEARCH"),
        "filtering should show SEARCH chip"
    );
    assert!(screen.contains("tokio-rs/tokio"));
    assert!(
        !screen.contains("sharkdp/bat"),
        "non-matching result should be filtered out"
    );

    // Esc cancels the in-progress filter (restores full list).
    app.handle_key(key(KeyCode::Esc));
    let rows = render(&mut app, 100, 30);
    assert!(rows.join("\n").contains("sharkdp/bat"));
}

#[test]
fn keybinds_popup_walks_modes_and_closes_without_residue() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('?')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("keybindings"), "popup title missing");
    // Every mode's chip sits in the sidebar; BROWSE is active.
    assert!(screen.contains("BROWSE"), "browse chip missing");
    assert!(screen.contains("LEADER"), "leader chip missing");
    assert!(screen.contains("VISUAL"), "visual chip missing");
    println!("{screen}");

    // Tab walks the modes; the leader table renders its bindings.
    for _ in 0..5 {
        app.handle_key(key(KeyCode::Tab));
    }
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("clear marks"), "leader bindings missing");

    app.handle_key(key(KeyCode::Esc));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(!screen.contains("keybindings"), "popup residue after close");
}

#[test]
fn command_line_filters_and_runs_settings() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(':')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("settings"), "command option missing");
    assert!(screen.contains("clone"), "clone option missing");
    assert!(screen.contains("INSERT"), "command line is a text input");
    println!("{screen}");

    for c in "set".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("settings"));
    assert!(
        !screen.contains("clone the selected"),
        "filtered-out command should disappear"
    );

    app.handle_key(key(KeyCode::Enter)); // → settings popup
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("settings"), "settings popup missing");
    assert!(screen.contains("editor"), "editor section missing");
    assert!(screen.contains("theme"), "theme section missing");
    assert!(screen.contains("cache"), "cache section missing");
    println!("{screen}");
}

#[test]
fn settings_popup_switches_tabs() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(':')));
    for c in "settings".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    app.handle_key(key(KeyCode::Tab)); // editor → theme
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("catppuccin-mocha"), "theme value missing");

    app.handle_key(key(KeyCode::Esc)); // close
    let rows = render(&mut app, 100, 30);
    assert!(!rows.join("\n").contains("settings"), "residue after close");
}

#[test]
fn clone_wizard_walks_three_screens() {
    let mut app = browsing_app();
    // Mark one repo in VISUAL, exit, then :clone.
    app.handle_key(key(KeyCode::Char('v')));
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('v')));

    app.handle_key(key(KeyCode::Char(':')));
    for c in "clone".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("clone — 1/3 repos"), "screen 1 missing");
    assert!(screen.contains("● ratatui"), "marked repo missing");
    assert!(screen.contains("Next →"), "next button missing");
    println!("{screen}");

    app.handle_key(key(KeyCode::Tab)); // list → buttons
    app.handle_key(key(KeyCode::Enter)); // next → destination
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("clone — 2/3 destination"),
        "screen 2 missing"
    );
    assert!(screen.contains("dest:"), "dest path missing");
    println!("{screen}");

    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Enter)); // next → summary
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("clone — 3/3 summary"), "screen 3 missing");
    assert!(screen.contains("git clone"), "clone command missing");
    println!("{screen}");

    // Esc closes the entire wizard from any screen.
    app.handle_key(key(KeyCode::Esc));
    let rows = render(&mut app, 100, 30);
    assert!(!rows.join("\n").contains("clone —"), "wizard residue");
}

/// v1.4 (plans/0014 #1): org expansion carries listing metadata into
/// the wizard — recently pushed first, undated by name after, archived
/// rows carry a dim note.
#[test]
fn clone_wizard_sorts_by_pushed_and_marks_archived() {
    let mut app = browsing_app();
    app.handle_app_event(rootle::event::AppEvent::CloneExpanded {
        repos: vec![
            rootle_provider::RepoInfo::bare("ratatui/bare"),
            rootle_provider::RepoInfo {
                name: "ratatui/old".into(),
                pushed_at: Some("2026-01-05T09:00:00Z".into()),
                ..Default::default()
            },
            rootle_provider::RepoInfo {
                name: "ratatui/fresh".into(),
                pushed_at: Some("2026-08-20T10:11:12Z".into()),
                ..Default::default()
            },
            rootle_provider::RepoInfo {
                name: "ratatui/mothballed".into(),
                archived: true,
                pushed_at: Some("2025-06-01T00:00:00Z".into()),
                ..Default::default()
            },
        ],
        errors: vec![],
    });
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("clone — 1/3 repos"), "wizard did not open");
    let pos = |needle: &str| screen.find(needle).unwrap_or(usize::MAX);
    assert!(
        pos("ratatui/fresh") < pos("ratatui/old")
            && pos("ratatui/old") < pos("ratatui/mothballed")
            && pos("ratatui/mothballed") < pos("ratatui/bare"),
        "expected pushed-desc, undated last:\n{screen}"
    );
    assert!(
        screen.contains("archived · 2025-06-01"),
        "archived note missing:\n{screen}"
    );
    println!("{screen}");
}

#[test]
fn leader_yank_toasts_hit_url_in_search_view() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter)); // submit → results focused

    // Leader works over the search view; ␣ y toasts the hit's URL.
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('y')));
    let rows = render(&mut app, 120, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("nothing to yank"),
        "yank toast missing:\n{screen}"
    );
}

#[test]
fn leader_chip_and_hints_show_over_search_view() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('f'))); // open the file-find view
    for c in "term".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter)); // submit → results focus
    let screen = render(&mut app, 120, 30).join("\n");
    assert!(screen.contains("find file"), "search view missing");

    // ␣ raises the leader layer over the view — the modeline must
    // flip to the LEADER chip with the leader hints (wide enough that
    // none drop off the tail).
    app.handle_key(key(KeyCode::Char(' ')));
    let screen = render(&mut app, 200, 30).join("\n");
    assert!(
        screen.contains("LEADER"),
        "leader chip missing over the search view:\n{screen}"
    );
    assert!(screen.contains("yank"), "leader hints missing:\n{screen}");
    // plans/0016 M1: revision keys are in the row.
    assert!(screen.contains("branches"), "refs hint missing:\n{screen}");
    assert!(
        screen.contains("preview"),
        "preview-submode hint missing:\n{screen}"
    );

    // Esc drops the layer; the view stays open with its own chip.
    app.handle_key(key(KeyCode::Esc));
    let screen = render(&mut app, 120, 30).join("\n");
    assert!(
        screen.contains("find file"),
        "view should survive leader Esc"
    );
    assert!(!screen.contains("LEADER"), "leader chip should drop");
}

#[test]
fn scrollable_popups_show_a_border_scrollbar() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('?')));
    // Short terminal: the active mode's bindings overflow the popup.
    let rows = render(&mut app, 100, 14);
    let screen = rows.join("\n");
    assert!(screen.contains('┃'), "scrollbar thumb missing:\n{screen}");

    // Scrolling moves the thumb.
    for _ in 0..4 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    let rows = render(&mut app, 100, 14);
    assert!(
        rows.join("\n").contains('┃'),
        "thumb should persist mid-scroll"
    );
}
