use super::*;

/// plans/0012 M1 eye candy: the query field restyles grammar tokens —
/// qualifier keys take the keyword color, quoted literals the string
/// color, negation markers the invalid color. Cell-level proof, and
/// plain terms keep the default text color (no bleed).
#[test]
fn query_field_restyles_grammar_tokens() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    for c in "render -legacy language:rust".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let backend = TestBackend::new(140, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            app.render(f, area);
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    let syntax = rootle::theme::Theme::catppuccin_mocha().syntax;
    let row_y = (0..buf.area.height).find(|y| {
        (0..buf.area.width)
            .map(|x| buf[(x, *y)].symbol().to_string())
            .collect::<String>()
            .contains("render -legacy language:rust")
    });
    let y = row_y.expect("the query field row");
    let text: String = (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect();
    let fg_at = |needle: &str| {
        // text is one symbol per cell; ❯ is multi-byte, so the cell
        // index is the CHAR count before the needle, not its byte offset.
        let byte_pos = text.find(needle).expect(needle);
        let x = text[..byte_pos].chars().count() as u16;
        buf[(x, y)].fg
    };
    assert_eq!(fg_at("language"), syntax.keyword, "qualifier key color");
    assert_eq!(fg_at("-legacy"), syntax.invalid, "negation marker color");
    assert_eq!(fg_at("rust"), syntax.string, "qualifier value color");
    // The plain term keeps the default text color — nothing bleeds.
    let text_fg = rootle::theme::Theme::catppuccin_mocha().semantic.text;
    assert_eq!(fg_at("render"), text_fg, "plain term color");
}

#[test]
fn leader_f_opens_find_view_and_enter_shows_mock_results() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('f')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");

    // Full-screen view: field row on top, results box below.
    assert!(screen.contains("find file"), "view title missing");
    assert!(screen.contains("query"), "query field missing");
    assert!(screen.contains("scope"), "scope field missing");
    assert!(screen.contains("extension"), "extension field missing");
    assert!(
        screen.contains("repo:ratatui/ratatui"),
        "scope label missing"
    );
    assert!(screen.contains("INSERT"), "query should land in INSERT");
    println!("{screen}");

    // Type a query, Enter runs the (mock) search and focuses results.
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("src/query/parser.rs"), "hit path missing");
    assert!(
        screen.contains("tests/query_roundtrip.rs"),
        "second hit missing"
    );
    assert!(
        screen.contains("pub fn parse(input: &str)"),
        "preview line missing"
    );
    assert!(screen.contains("BROWSE"), "results focus = BROWSE chip");
    println!("{screen}");
}

#[test]
fn leader_g_opens_grep_view_with_scope_radio_popup() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    let rows = render(&mut app, 100, 30);
    assert!(rows.join("\n").contains("grep"), "grep title missing");

    // Tab to the scope field, Enter opens the radio popup.
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Enter));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("(•) current repo"),
        "radio selected missing"
    );
    assert!(screen.contains("( ) current org"), "org option missing");
    println!("{screen}");

    // j j → repo → org → global (radio follows the cursor),
    // Enter commits by closing; modeline context follows.
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Char('j')));
    app.handle_key(key(KeyCode::Enter));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("global"), "scope should switch to global");
    assert!(screen.contains("grep · global"), "modeline context missing");
}

#[test]
fn search_view_results_support_slash_filter() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter)); // submit → results focused

    app.handle_key(key(KeyCode::Char('/')));
    for c in "terminal".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("SEARCH"),
        "filtering should show SEARCH chip"
    );
    assert!(screen.contains("src/terminal.rs"));
    assert!(
        !screen.contains("src/widgets/list.rs"),
        "non-matching hit should be filtered out"
    );

    // Enter on the remaining hit expands its file; a second Enter
    // opens the editor on it (mock bytes).
    app.handle_key(key(KeyCode::Esc)); // Esc cancels filter → full list
    app.handle_key(key(KeyCode::Enter)); // expand the selected hit
    app.handle_key(key(KeyCode::Enter)); // open it in the editor
    assert!(
        app.take_editor_job().is_some(),
        "Enter on a hit should prepare an editor job"
    );
}

/// plans/0012 M1 honesty chips: client-subtracted hits and
/// inexpressible tokens are named in the results title.
#[test]
fn grammar_chips_say_what_was_filtered() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    app.handle_action(rootle::action::Action::GlobalSearchSubmitted {
        kind: rootle::components::global_search::SearchKind::Grep,
        query: "render -legacy language:cobol".into(),
        scope: "global".into(),
        extension: String::new(),
    });
    app.handle_action(rootle::action::Action::GlobalSearchResults {
        hits: vec![rootle::components::global_search::SearchHit::plain(
            "ratatui/ratatui",
            "src/render.rs",
            3,
            vec![(3, "fn render() {".to_string())],
            1,
            String::new(),
        )],
        clipped: false,
        index: None,
        client_filtered: 2,
        unfiltered: vec!["language:cobol".into()],
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("filtered 2"),
        "filtered chip missing: {screen}"
    );
    assert!(
        screen.contains("unfiltered: language:cobol"),
        "unfiltered chip missing: {screen}"
    );
    println!("{screen}");
}

#[test]
fn delayed_hits_keep_submitted_query_highlighting_after_input_edits() {
    use rootle::action::Action;
    use rootle::components::global_search::{RawHit, SearchKind};
    let mut app = browsing_app();
    app.handle_action(Action::LeaderGrep);
    app.handle_action(Action::GlobalSearchSubmitted {
        kind: SearchKind::Grep,
        query: "needle".into(),
        scope: "global".into(),
        extension: String::new(),
    });
    let request = app.search_request().unwrap().clone();
    app.handle_key(key(KeyCode::Tab));
    for character in "changed".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_app_event(rootle::event::AppEvent::GlobalSearchResults {
        gen_id: request.generation,
        hits: vec![RawHit {
            repo: "ratatui/ratatui".into(),
            path: "result.rs".into(),
            sha: "blob".into(),
            branch: "main".into(),
            line: 1,
            preview: vec![(1, "needle changed".into())],
            match_count: 1,
            stale: false,
        }],
        clipped: false,
        index: None,
        client_filtered: 0,
        unfiltered: vec![],
    });
    assert_eq!(app.snapshot()["search"]["request"]["query"], "needle");
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| app.render(frame, frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let (column, row) = (0..buffer.area.height)
        .find_map(|row| {
            let text: String = (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect();
            text.find("needle changed")
                .map(|byte| (text[..byte].chars().count() as u16, row))
        })
        .expect("delayed hit visible");
    let highlight = rootle::theme::Theme::catppuccin_mocha()
        .semantic
        .search_match;
    assert_eq!(buffer[(column, row)].bg, highlight);
    assert_ne!(buffer[(column + 7, row)].bg, highlight);
}

#[test]
fn closed_and_superseded_search_requests_cannot_overwrite_the_new_view() {
    use rootle::action::Action;
    use rootle::components::global_search::SearchKind;
    use rootle::event::AppEvent;
    let mut app = browsing_app();
    app.handle_action(Action::LeaderGrep);
    app.handle_action(Action::GlobalSearchSubmitted {
        kind: SearchKind::Grep,
        query: "obsolete-query".into(),
        scope: "global".into(),
        extension: String::new(),
    });
    let obsolete = app.search_request().unwrap().generation;
    app.handle_action(Action::CloseSearchView);
    app.handle_action(Action::LeaderGrep);
    app.handle_action(Action::GlobalSearchSubmitted {
        kind: SearchKind::Grep,
        query: "current-query".into(),
        scope: "global".into(),
        extension: String::new(),
    });
    let current = app.snapshot();
    app.handle_app_event(AppEvent::GlobalSearchFailed {
        gen_id: obsolete,
        error: "obsolete failure".into(),
    });
    app.handle_app_event(AppEvent::GlobalSearchResults {
        gen_id: obsolete,
        hits: vec![],
        clipped: true,
        index: None,
        client_filtered: 10,
        unfiltered: vec!["obsolete".into()],
    });
    assert_eq!(app.snapshot(), current);
}

#[test]
fn failure_arrival_preserves_the_active_query_editor() {
    use rootle::action::Action;
    let mut app = browsing_app();
    app.handle_action(Action::LeaderGrep);
    app.handle_action(Action::GlobalSearchSubmitted {
        kind: rootle::components::global_search::SearchKind::Grep,
        query: "submitted".into(),
        scope: "global".into(),
        extension: String::new(),
    });
    let generation = app.search_request().unwrap().generation;
    app.handle_key(key(KeyCode::Tab));
    for character in "draft".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_app_event(rootle::event::AppEvent::GlobalSearchFailed {
        gen_id: generation,
        error: "current request failed".into(),
    });
    app.handle_key(key(KeyCode::Char('q')));
    assert_eq!(app.snapshot()["mode"], "INSERT");
    assert_eq!(app.snapshot()["search"]["request"]["query"], "submitted");
    assert!(render(&mut app, 100, 30).join("\n").contains("draftq"));
}
