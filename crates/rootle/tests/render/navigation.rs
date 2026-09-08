use super::*;

#[test]
fn renders_three_panes_modeline_and_popup() {
    // Seeded orgs behind the popup; the popup itself opened via ␣ s
    // (auto-open happens only on a fresh state).
    let mut app = app_with_orgs(&["ratatui", "tokio-rs", "helix-editor"]);
    app.handle_action(rootle::action::Action::LeaderSearch);
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");

    // Launch: search popup is open over the browser.
    assert!(screen.contains("search offline"), "popup title missing");
    assert!(screen.contains("INSERT"), "should land in INSERT mode");
    assert!(screen.contains("tab focus"), "popup hint row missing");

    // Browser visible behind the popup: orgs column + repos column.
    assert!(screen.contains("ratatui/"), "repos pane missing");

    // Print the frame for eyeballing (cargo test -- --nocapture).
    println!("{screen}");
}

#[test]
fn popup_close_leaves_no_lingering_cells() {
    let mut app = test_app();
    let _ = render(&mut app, 100, 30); // popup open

    // Esc twice: INSERT → NORMAL → close popup.
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc));
    let after = render(&mut app, 100, 30);
    let screen = after.join("\n");

    assert!(
        !screen.contains("search github"),
        "popup residue after close"
    );
    assert!(screen.contains("BROWSE"), "should return to BROWSE");

    // The area where the popup was must show panes again, not blanks.
    let middle = &after[15];
    assert!(
        middle.contains('│') || middle.contains('╮') || middle.contains('╯'),
        "middle row lost pane borders after popup close"
    );
}

#[test]
fn resize_keeps_modeline_on_last_row() {
    let mut app = browsing_app(); // popup closed + org repos loaded
    for (w, h) in [(80, 24), (120, 40), (40, 10)] {
        let rows = render(&mut app, w, h);
        let last = rows.last().unwrap();
        assert!(
            last.contains("BROWSE"),
            "modeline missing on last row at {w}x{h}"
        );
    }
}

#[test]
fn searching_mode_filters_incrementally() {
    let mut app = browsing_app(); // popup closed + org repos loaded → repos pane
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Char('w'))); // "website"
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("SEARCH"));
    assert!(screen.contains("/w"), "filter not shown in pane title");
    assert!(screen.contains("ratatui-website"));
    assert!(
        !screen.contains("comfy-table"),
        "non-matching entry should be filtered out"
    );
}

#[test]
fn h_moves_focus_to_parent_and_browsing_it_cascades() {
    let mut app = browsing_app(); // popup closed + org repos loaded

    // Drill into a repo: focus is now on the repo's root dir column.
    app.handle_key(key(KeyCode::Char('l')));
    let rows = render(&mut app, 100, 30);
    assert!(
        rows.join("\n").contains("Cargo.toml"),
        "should see repo root"
    );

    // h: focus moves left into the repos column; j selects
    // ratatui-website. Repo trees arrive from the API — the child
    // column appears only when the tree lands.
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_key(key(KeyCode::Char('j')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        !screen.contains("Cargo.toml"),
        "no child column before the tree arrives"
    );
    app.handle_key(key(KeyCode::Char('l')));
    app.handle_action(rootle::action::Action::TreeLoaded {
        request: app.tree_request().unwrap().clone(),
        entries: ratatui_tree(),
        truncated: false,
        branch: "main".into(),
    });
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("Cargo.toml"),
        "child column should appear with the tree"
    );
    // h twice: root pane (auto-entered on tree load) → repos → orgs.
    // j picks tokio-rs. Org repos now arrive from the API — inject the
    // response (offline app never spawns workers).
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_key(key(KeyCode::Char('j')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(rows[0].ends_with('┐'), "orgs should fold to full width");
    assert!(
        !screen.contains("axum/"),
        "folded view hides the repos column"
    );

    // l on an org triggers LoadOrgRepos; the response installs the
    // repos level — no stale ratatui entries.
    app.handle_key(key(KeyCode::Char('l')));
    app.handle_action(rootle::action::Action::OrgReposLoaded {
        request: app.owner_request().unwrap().clone(),
        repos: vec![
            "tokio".into(),
            "axum".into(),
            "hyper".into(),
            "tracing".into(),
            "bytes".into(),
        ],
    });
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("axum/"), "org switch should load repos");
    assert!(!screen.contains("comfy-table"), "stale child column leaked");
}

#[test]
fn drilling_into_dir_uses_correct_relative_path() {
    let mut app = browsing_app(); // popup closed + org repos loaded → repos pane
    app.handle_key(key(KeyCode::Char('l'))); // into ratatui root
    // Dirs sort alphabetically: docs/, examples/, src/ — j twice to src.
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Char('l'))); // into src/
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    // The src/ bucket: widgets/, layout/, lib.rs in the center pane.
    assert!(screen.contains("widgets/"), "src/ children missing");
    assert!(screen.contains("lib.rs"), "src/ children missing");
    // Hovering a file shows its blob meta (sha + size) until milestone 5.
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Char('j'))); // hover lib.rs
    let rows = render(&mut app, 100, 30);
    assert!(
        rows.join("\n").contains("blob abc1234"),
        "lib.rs blob meta missing after drilling into src/"
    );
}

#[test]
fn org_level_folds_to_single_pane() {
    let mut app = browsing_app(); // popup closed + org repos loaded

    // h until focus reaches the orgs column (top level).
    app.handle_key(key(KeyCode::Char('h')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");

    // Single folded pane: orgs visible, no repo column border beside it.
    assert!(screen.contains("tokio-rs/"));
    // A folded single pane spans nearly full width: orgs title starts at
    // the left edge and its right border sits at the far right.
    let top = &rows[0];
    assert!(top.starts_with('┌'), "folded pane should start at x=0");
    assert!(
        top.ends_with('┐'),
        "folded pane should reach the right edge"
    );
}

#[test]
fn visual_mode_marks_repos_with_checkboxes() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('v')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("VISUAL"), "visual chip missing");
    assert!(screen.contains("○"), "checkboxes missing");

    // Focus is on the repos pane (helper lands there); mark two repos.
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Char(' ')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("●"), "checked row missing");
    println!("{screen}");

    app.handle_key(key(KeyCode::Char('v'))); // exit visual
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("BROWSE"), "should return to BROWSE");
    assert!(!screen.contains("○"), "checkboxes should disappear");
}

#[test]
fn unfocused_parent_pane_is_dimmed() {
    use ratatui::style::Modifier;
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // focus repo root; repos pane unfocused
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            app.render(f, area);
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    // An unselected repo entry in the unfocused parent pane.
    let row = (0..buf.area.height)
        .find(|&y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("templates/")
        })
        .expect("repo entry row should render");
    let cell = (0..buf.area.width)
        .map(|x| buf[(x, row)].clone())
        .find(|c| c.symbol() == "t")
        .expect("entry cell");
    assert!(
        cell.modifier.contains(Modifier::DIM),
        "unfocused dir entry should be dimmed, got {:?}",
        cell.modifier
    );
}

#[test]
fn panes_get_scrollbars_when_they_overflow() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // drill into the repo root (6 entries)
    // Terminal too short for the root listing → scrollbar appears.
    let rows = render(&mut app, 60, 7);
    assert!(
        rows.join("\n").contains('┃'),
        "overflowing panes should show a scrollbar"
    );
}
