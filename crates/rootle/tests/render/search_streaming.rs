use super::*;

#[test]
fn streamed_batches_merge_and_metadata_final_keeps_the_set() {
    let mut app = browsing_app();
    // Open the grep view and submit (offline: the mock path is bypassed
    // by injecting the streamed actions directly).
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    app.handle_action(rootle::action::Action::GlobalSearchSubmitted {
        kind: rootle::components::global_search::SearchKind::Grep,
        query: "hit".into(),
        scope: "global".into(),
        extension: String::new(),
    });

    // Two streamed batches: a new file, then a second file, then a
    // batch hitting the SAME file as the first — folded, not duped.
    let mk = |path: &str, line: u32, count: u32| {
        rootle::components::global_search::SearchHit::plain(
            "ratatui/ratatui",
            path,
            line,
            vec![(line, "let hit = 1;".to_string())],
            count,
            String::new(),
        )
    };
    app.handle_action(rootle::action::Action::GlobalSearchDelta {
        hits: vec![mk("src/a.rs", 3, 1), mk("src/b.rs", 10, 1)],
    });
    app.handle_action(rootle::action::Action::GlobalSearchDelta {
        hits: vec![mk("src/a.rs", 42, 1)],
    });
    // Metadata-only final (provider streamed): the set stands, clipped
    // applies.
    app.handle_action(rootle::action::Action::GlobalSearchResults {
        hits: vec![],
        clipped: true,
        index: Some("2026-08-20T14:00:00Z".into()),
        client_filtered: 0,
        unfiltered: vec![],
    });

    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("src/a.rs"), "first batch hit kept");
    assert!(screen.contains("src/b.rs"), "second batch hit kept");
    // a.rs appears once — merged with its second batch, not appended.
    assert_eq!(
        screen.matches("src/a.rs").count(),
        1,
        "same-file batch merges into one block"
    );
    assert!(screen.contains("42"), "merged region line visible");
    assert!(screen.contains("clipped"), "metadata final applies clipped");
    assert!(
        screen.contains("index 2026-08-20T14:00"),
        "v1.3 index badge in title: {screen}"
    );
    assert!(!screen.contains("streaming"), "final clears pending");
}

/// Boxed result blocks: the filename rides the top rule, the badge
/// closes it, rails wrap the guttered match lines; the selected box's
/// rails carry the accent color.
#[test]
fn grep_results_render_as_decorated_boxes() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    app.handle_action(rootle::action::Action::GlobalSearchSubmitted {
        kind: rootle::components::global_search::SearchKind::Grep,
        query: "render".into(),
        scope: "global".into(),
        extension: String::new(),
    });
    app.handle_action(rootle::action::Action::GlobalSearchResults {
        hits: vec![
            rootle::components::global_search::SearchHit::plain(
                "ratatui/ratatui",
                "src/render.rs",
                3,
                vec![(3, "fn render() {".to_string())],
                2,
                String::new(),
            ),
            // Path-only hit: a two-line capsule, no content rails.
            rootle::components::global_search::SearchHit::plain(
                "ratatui/ratatui",
                "src/draw.rs",
                7,
                vec![],
                0,
                String::new(),
            ),
        ],
        clipped: false,
        index: None,
        client_filtered: 0,
        unfiltered: vec![],
    });
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("┌─ ratatui/ratatui/src/render.rs"),
        "box title missing:\n{screen}"
    );
    assert!(
        screen.contains("2 matches ─┐"),
        "badge not in the top rule:\n{screen}"
    );
    assert!(
        screen.contains("│   3 │ fn render() {"),
        "railed gutter content missing:\n{screen}"
    );
    let capsule = rows
        .iter()
        .position(|r| r.contains("src/draw.rs"))
        .expect("path-only hit");
    assert!(rows[capsule].contains("┌─"), "capsule top:\n{screen}");
    assert!(
        rows[capsule + 1].contains("└"),
        "capsule bottom directly under the title:\n{}",
        rows[capsule + 1]
    );
    println!("{screen}");
}

/// plans/0012 M3: facet chips appear while a search streams and
/// re-count as batches land.
#[test]
fn facet_chips_appear_and_update_as_batches_stream() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    app.handle_action(rootle::action::Action::GlobalSearchSubmitted {
        kind: rootle::components::global_search::SearchKind::Grep,
        query: "zzqqx-no-mock-match".into(),
        scope: "global".into(),
        extension: String::new(),
    });
    let mk = |repo: &str, path: &str| {
        rootle::components::global_search::SearchHit::plain(
            repo,
            path,
            3,
            vec![(3, "let zzqqx = 1;".to_string())],
            1,
            String::new(),
        )
    };

    // First streamed batch lands — the chip row appears with
    // per-batch counts while the set is still growing.
    app.handle_action(rootle::action::Action::GlobalSearchDelta {
        hits: vec![
            mk("local/alpha", "src/one.rs"),
            mk("local/beta", "docs/x.md"),
        ],
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains(" results — 2 "),
        "two hits so far: {screen}"
    );
    for chip in ["local/alpha·1", "local/beta·1", "markdown·1", "rust·1"] {
        assert!(screen.contains(chip), "chip {chip} missing: {screen}");
    }

    // Second batch: another alpha rust hit — its counts climb, the
    // chip order re-sorts (alpha first).
    app.handle_action(rootle::action::Action::GlobalSearchDelta {
        hits: vec![mk("local/alpha", "src/two.rs")],
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains(" results — 3 "), "the set grows: {screen}");
    assert!(screen.contains("local/alpha·2"), "count climbs: {screen}");
    let alpha = screen.find("local/alpha·2").expect("chip");
    let beta = screen.find("local/beta·1").expect("chip");
    assert!(alpha < beta, "most-hits-first within the repo group");
}

/// plans/0012 M3: Enter on a chip commits the facet (list narrows),
/// Enter again restores, and closing the view leaves no chip residue.
#[test]
fn facet_selection_narrows_clears_and_leaves_no_residue() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    app.handle_action(rootle::action::Action::GlobalSearchSubmitted {
        kind: rootle::components::global_search::SearchKind::Grep,
        query: "hit".into(),
        scope: "global".into(),
        extension: String::new(),
    });
    let mk = |repo: &str, path: &str| {
        rootle::components::global_search::SearchHit::plain(
            repo,
            path,
            3,
            vec![(3, "let hit = 1;".to_string())],
            1,
            String::new(),
        )
    };
    app.handle_action(rootle::action::Action::GlobalSearchResults {
        hits: vec![
            mk("local/alpha", "src/one.rs"),
            mk("local/beta", "docs/x.md"),
        ],
        clipped: false,
        index: None,
        client_filtered: 0,
        unfiltered: vec![],
    });
    // Tab to the chip row from results: results → query → scope →
    // extension → facets.
    for _ in 0..4 {
        app.handle_key(key(KeyCode::Tab));
    }
    // Cursor 0 = local/alpha (most hits first). Enter commits it.
    app.handle_key(key(KeyCode::Enter));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("src/one.rs"), "faceted hit stays: {screen}");
    assert!(
        !screen.contains("docs/x.md"),
        "other repo's hit is filtered out"
    );

    // Enter on the active chip restores the full accumulated set.
    app.handle_key(key(KeyCode::Enter));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("docs/x.md"), "full set restored: {screen}");

    // Closing the view puts the browser back with no chip residue.
    app.handle_key(key(KeyCode::Esc)); // Esc from the chip row closes
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("ratatui"), "browser restored");
    assert!(
        !screen.contains("local/alpha·"),
        "no chip residue after leaving the view: {screen}"
    );
}

#[test]
fn stale_hit_shows_chip_until_located() {
    // v1.1: a search/code item with located=false renders a `stale`
    // chip instead of line numbers; HitContextLoaded clears it.
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    let mut stale = rootle::components::global_search::SearchHit::plain(
        "owner/repo",
        "src/place.rs",
        1,
        vec![],
        0,
        String::new(),
    );
    stale.sha = "deadbee".into();
    stale.stale = true;
    app.handle_action(rootle::action::Action::GlobalSearchResults {
        hits: vec![stale],
        clipped: false,
        index: None,
        client_filtered: 0,
        unfiltered: vec![],
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("stale"),
        "stale chip should render:\n{screen}"
    );
    // Context lands (located client-side): chip clears, line shows.
    app.handle_action(rootle::action::Action::HitContextLoaded {
        repo: "owner/repo".into(),
        path: "src/place.rs".into(),
        sha: "deadbee".into(),
        line: 7,
        preview: vec![(7, ratatui::text::Line::from("needle here"))],
        match_count: 1,
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        !screen.contains("stale"),
        "chip clears after locate:\n{screen}"
    );
    assert!(screen.contains("needle here"), "preview renders:\n{screen}");
}

#[test]
fn unlocatable_hit_flips_from_stale_to_its_own_chip() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    let mut hit = rootle::components::global_search::SearchHit::plain(
        "owner/repo",
        "src/place.rs",
        1,
        vec![],
        0,
        String::new(),
    );
    hit.sha = "deadbee".into();
    hit.stale = true;
    app.handle_action(rootle::action::Action::GlobalSearchResults {
        hits: vec![hit],
        clipped: false,
        index: None,
        client_filtered: 0,
        unfiltered: vec![],
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("stale"), "stale chip renders:\n{screen}");

    // The blob arrived but the match text isn't in it (plans/0008 §4):
    // the hit stops pretending it's just stale.
    app.handle_action(rootle::action::Action::HitContextMissing {
        sha: "deadbee".into(),
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("unlocatable"),
        "unlocatable chip renders:\n{screen}"
    );
    assert!(!screen.contains("stale"), "stale chip clears:\n{screen}");
}

#[test]
fn clipped_result_set_says_so_in_the_title() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    let hit = rootle::components::global_search::SearchHit::plain(
        "owner/repo",
        "src/place.rs",
        1,
        vec![],
        0,
        String::new(),
    );
    app.handle_action(rootle::action::Action::GlobalSearchResults {
        hits: vec![hit],
        clipped: true,
        index: None,
        client_filtered: 0,
        unfiltered: vec![],
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("clipped"),
        "clipped note should render in the results title:\n{screen}"
    );
}
