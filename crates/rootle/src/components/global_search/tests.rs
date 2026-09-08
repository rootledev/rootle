use super::mock;
use super::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn view() -> GlobalSearch {
    GlobalSearch::new(
        SearchKind::Grep,
        Some("ratatui/ratatui".into()),
        Some("ratatui".into()),
        None,
        None,
    )
}

fn submit(view: &mut GlobalSearch, query: &str) {
    for c in query.chars() {
        view.handle_key(key(KeyCode::Char(c)));
    }
    let action = view.handle_key(key(KeyCode::Enter));
    let Action::GlobalSearchSubmitted {
        kind,
        query: submitted,
        scope,
        extension,
    } = action
    else {
        panic!("query must submit");
    };
    view.start_request(crate::request::ContentSearchRequest {
        generation: Default::default(),
        kind,
        query: submitted,
        scope,
        extension,
    });
    view.update(&Action::GlobalSearchResults {
        hits: mock::hits(SearchKind::Grep, query, ""),
        clipped: false,
        index: None,
        client_filtered: 0,
        unfiltered: vec![],
    });
}

#[test]
fn tab_cycles_all_four_focus_targets() {
    let mut v = view();
    assert_eq!(v.focus, Focus::Query);
    v.handle_key(key(KeyCode::Tab));
    assert_eq!(v.focus, Focus::Scope);
    v.handle_key(key(KeyCode::Tab));
    assert_eq!(v.focus, Focus::Extension);
    v.handle_key(key(KeyCode::Tab));
    assert_eq!(v.focus, Focus::Results);
    v.handle_key(key(KeyCode::Tab));
    assert_eq!(v.focus, Focus::Query);
    v.handle_key(key(KeyCode::BackTab));
    assert_eq!(v.focus, Focus::Results);
}

#[test]
fn enter_in_query_submits_and_focuses_results() {
    let mut v = view();
    submit(&mut v, "query");
    assert_eq!(v.focus, Focus::Results);
    assert_eq!(v.hits.len(), 4);
}

#[test]
fn scope_popup_radio_follows_cursor_and_esc_reverts() {
    let mut v = view();
    v.handle_key(key(KeyCode::Tab)); // scope focused
    v.handle_key(key(KeyCode::Enter));
    assert!(v.scope_popup);
    // Radio follows the cursor down the waterfall: repo → org → global.
    v.handle_key(key(KeyCode::Char('j')));
    assert_eq!(v.scope, Scope::Org);
    v.handle_key(key(KeyCode::Char('j')));
    assert_eq!(v.scope, Scope::Global);
    v.handle_key(key(KeyCode::Esc)); // revert to the pre-popup scope
    assert!(!v.scope_popup);
    assert_eq!(v.scope, Scope::Repo);

    // Enter commits wherever the radio stands.
    v.handle_key(key(KeyCode::Enter));
    v.handle_key(key(KeyCode::Char('j')));
    v.handle_key(key(KeyCode::Char('j')));
    v.handle_key(key(KeyCode::Enter));
    assert!(!v.scope_popup);
    assert_eq!(v.scope, Scope::Global);
}

#[test]
fn repo_and_org_scopes_disabled_without_context() {
    let mut v = GlobalSearch::new(SearchKind::FileFind, None, None, None, None);
    assert_eq!(v.scope, Scope::Global);
    v.handle_key(key(KeyCode::Tab));
    v.handle_key(key(KeyCode::Enter)); // open popup
    v.handle_key(key(KeyCode::Char('j'))); // wraps: repo + org skipped
    assert_eq!(v.scope_cursor, 2); // global stays the only target
    v.handle_key(key(KeyCode::Enter));
    assert_eq!(v.scope, Scope::Global);
}

#[test]
fn scope_waterfalls_from_browser_context() {
    // Repo open → Repo; only org → Org; nothing → Global.
    let v = GlobalSearch::new(
        SearchKind::Grep,
        Some("ratatui/ratatui".into()),
        Some("ratatui".into()),
        None,
        None,
    );
    assert_eq!(v.scope, Scope::Repo);
    let v = GlobalSearch::new(SearchKind::Grep, None, Some("ratatui".into()), None, None);
    assert_eq!(v.scope, Scope::Org);
    assert_eq!(v.scope_label(), "org:ratatui");
    let v = GlobalSearch::new(SearchKind::Grep, None, None, None, None);
    assert_eq!(v.scope, Scope::Global);
}

#[test]
fn slash_filter_narrows_and_esc_restores() {
    let mut v = view();
    submit(&mut v, "query");
    assert_eq!(v.focus, Focus::Results);
    v.handle_key(key(KeyCode::Char('/')));
    assert!(v.filtering);
    for c in "terminal".chars() {
        v.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(v.visible().len(), 1);
    assert_eq!(v.visible()[0].path, "src/terminal.rs");
    v.handle_key(key(KeyCode::Esc)); // cancel → pre-filter
    assert_eq!(v.visible().len(), 4);
}

#[test]
fn effective_mode_follows_focus_and_submode() {
    let mut v = view();
    assert_eq!(v.effective_mode(), Mode::Insert);
    v.query.submode = SubMode::Normal;
    assert_eq!(v.effective_mode(), Mode::Normal);
    v.handle_key(key(KeyCode::Tab)); // scope
    assert_eq!(v.effective_mode(), Mode::Browse);
}

#[test]
fn esc_closes_from_results() {
    let mut v = view();
    submit(&mut v, "query");
    let action = v.handle_key(key(KeyCode::Esc));
    assert_eq!(action, Action::CloseSearchView);
}

#[test]
fn enter_expands_hit_into_file_pane() {
    let mut v = view();
    submit(&mut v, "query"); // mock hits: body, no sha → render inline
    let action = v.handle_key(key(KeyCode::Enter));
    assert_eq!(action, Action::Noop, "mock hit needs no fetch");
    assert!(v.expanded.is_some(), "Enter expands the selected hit");
    let exp = v.expanded.as_ref().expect("expanded");
    assert_eq!(exp.hit.path, "src/widgets/list.rs");
    assert!(exp.loaded, "body hits render without a fetch");
    // The mock hit's line (42) exceeds its 9-line body — the
    // anchor clamps to the end of the file.
    assert_eq!(exp.preview.line(), Some(9));

    // Esc folds straight back; the selection is where it was.
    assert_eq!(v.handle_key(key(KeyCode::Esc)), Action::Noop);
    assert!(v.expanded.is_none());
    assert_eq!(
        v.selected_hit().map(|h| h.path.clone()),
        Some("src/widgets/list.rs".into())
    );
}

#[test]
fn real_hit_expands_with_fetch_and_anchor() {
    let mut v = view();
    submit(&mut v, "query");
    let mut hit = SearchHit::plain(
        "owner/repo",
        "src/place.rs",
        3,
        vec![(3, "the needle".to_string())],
        1,
        String::new(),
    );
    hit.sha = "cafebab".into();
    v.hits = vec![hit.clone()];
    // Enter: loading placeholder + the fetch action.
    let action = v.handle_key(key(KeyCode::Enter));
    match &action {
        Action::LoadHitFile { hit: asked } => assert_eq!(asked.sha, "cafebab"),
        other => panic!("expected LoadHitFile, got {other:?}"),
    }
    assert!(!v.expanded.as_ref().expect("expanded").loaded);

    // Blob lands: highlighted content, cursor at the anchor line,
    // title says repo/path:line.
    v.update(&Action::HitFileLoaded {
        repo: hit.repo.clone(),
        path: hit.path.clone(),
        sha: hit.sha.clone(),
        lang: "rust".into(),
        lines: (1..=9)
            .map(|i| ratatui::text::Line::from(format!("line {i}")))
            .collect(),
    });
    let exp = v.expanded.as_ref().expect("expanded");
    assert!(exp.loaded);
    assert_eq!(exp.preview.line(), Some(hit.line), "cursor at the anchor");
    assert_eq!(
        exp.preview.title,
        format!("owner/repo/{}:{}", hit.path, hit.line)
    );

    // j walks the file cursor; Enter opens the editor on the hit.
    v.handle_key(key(KeyCode::Char('j')));
    assert_eq!(
        v.expanded.as_ref().expect("expanded").preview.line(),
        Some(hit.line + 1)
    );
    match v.handle_key(key(KeyCode::Enter)) {
        Action::OpenSearchHit(opened) => assert_eq!(opened.path, hit.path),
        other => panic!("expected OpenSearchHit, got {other:?}"),
    }
}

#[test]
fn path_only_hit_expands_to_top_of_file() {
    let mut v = view();
    submit(&mut v, "query");
    // match_count 0, no known line (file-find shape).
    v.hits = vec![SearchHit::plain(
        "owner/repo",
        "docs/readme.md",
        0,
        vec![],
        0,
        String::new(),
    )];
    v.hits[0].sha = "beef00".into();
    v.handle_key(key(KeyCode::Enter));
    v.update(&Action::HitFileLoaded {
        repo: "owner/repo".into(),
        path: "docs/readme.md".into(),
        sha: "beef00".into(),
        lang: "markdown".into(),
        lines: (1..=4)
            .map(|i| ratatui::text::Line::from(format!("doc {i}")))
            .collect(),
    });
    let exp = v.expanded.as_ref().expect("expanded");
    assert_eq!(
        exp.preview.line(),
        Some(1),
        "unknown anchor falls back to top"
    );
    assert_eq!(
        exp.preview.title, "owner/repo/docs/readme.md",
        "no :0 suffix"
    );
    // Anchor past EOF clamps instead of panicking: fold back,
    // move the anchor, expand again.
    v.handle_key(key(KeyCode::Esc));
    v.hits[0].line = 99;
    v.handle_key(key(KeyCode::Enter));
    v.update(&Action::HitFileLoaded {
        repo: "owner/repo".into(),
        path: "docs/readme.md".into(),
        sha: "beef00".into(),
        lang: "markdown".into(),
        lines: (1..=4)
            .map(|i| ratatui::text::Line::from(format!("doc {i}")))
            .collect(),
    });
    assert_eq!(
        v.expanded.as_ref().expect("expanded").preview.line(),
        Some(4)
    );
}

#[test]
fn file_pane_find_session_delegates_to_preview() {
    let mut v = view();
    submit(&mut v, "query");
    v.handle_key(key(KeyCode::Enter)); // expand (mock body)
    // `/` opens FIND over the file; the modeline chip follows.
    v.handle_key(key(KeyCode::Char('/')));
    assert!(v.finding);
    assert_eq!(v.effective_mode(), Mode::Find);
    for c in "mock".chars() {
        v.handle_key(key(KeyCode::Char(c)));
    }
    let exp = v.expanded.as_ref().expect("expanded");
    assert!(exp.preview.find_active(), "preview holds the session");
    // Enter commits; n/N step, Esc-h still collapses afterwards.
    v.handle_key(key(KeyCode::Enter));
    assert!(!v.finding);
    v.handle_key(key(KeyCode::Char('n')));
    v.handle_key(key(KeyCode::Char('h')));
    assert!(v.expanded.is_none(), "h folds the pane back");
}

#[test]
fn scope_field_cycles_with_vim_motions() {
    let mut v = view();
    v.handle_key(key(KeyCode::Tab)); // scope focused
    assert_eq!(v.scope, Scope::Repo);
    v.handle_key(key(KeyCode::Char('j'))); // repo → org, no popup
    assert_eq!(v.scope, Scope::Org);
    v.handle_key(key(KeyCode::Char('j'))); // org → global
    assert_eq!(v.scope, Scope::Global);
    assert!(!v.scope_popup);
    v.handle_key(key(KeyCode::Char('k'))); // back to org
    assert_eq!(v.scope, Scope::Org);
    // Disabled scopes are skipped when no context is open.
    let mut v = GlobalSearch::new(SearchKind::Grep, None, None, None, None);
    v.handle_key(key(KeyCode::Tab));
    v.handle_key(key(KeyCode::Char('k')));
    assert_eq!(v.scope, Scope::Global);
}

/// Tab to the chip row (from wherever focus sits — after submit
/// that's Results, so the cycle wraps through the fields first).
fn focus_facets(v: &mut GlobalSearch) {
    for _ in 0..super::FOCUS_ORDER.len() + 1 {
        if v.focus == Focus::Facets {
            return;
        }
        v.handle_key(key(KeyCode::Tab));
    }
    panic!("tab never reached the chip row");
}

#[test]
fn tab_skips_the_chip_row_until_hits_land() {
    let mut v = view();
    // No hits yet: query → scope → extension → results, never
    // facets.
    for expected in [Focus::Scope, Focus::Extension, Focus::Results] {
        v.handle_key(key(KeyCode::Tab));
        assert_eq!(v.focus, expected);
    }
    v.handle_key(key(KeyCode::Tab));
    assert_eq!(v.focus, Focus::Query, "wraps without stopping on facets");

    // Mock grep hits: one repo, rust + markdown.
    submit(&mut v, "query");
    focus_facets(&mut v);
    assert_eq!(v.facets().len(), 3);
}

#[test]
fn facet_toggle_narrows_and_restores() {
    let mut v = view();
    submit(&mut v, "query"); // 4 hits: 3 rust + 1 markdown
    focus_facets(&mut v);
    // Chips: repo ratatui/ratatui·4, then rust·3, markdown·1.
    assert_eq!(v.facet_cursor, 0);
    v.handle_key(key(KeyCode::Char('l')));
    assert_eq!(v.facet_cursor, 1);
    v.handle_key(key(KeyCode::Enter)); // commit the rust facet
    assert_eq!(
        v.facet,
        Some(facets::FacetId {
            kind: facets::FacetKind::Lang,
            name: "rust".into(),
        })
    );
    assert_eq!(v.visible().len(), 3, "rust facet drops the markdown hit");
    v.handle_key(key(KeyCode::Enter)); // toggle the active chip off
    assert!(v.facet.is_none());
    assert_eq!(v.visible().len(), 4, "full accumulated set restored");
}

#[test]
fn facet_survives_streamed_batches_and_counts_climb() {
    let mut v = view();
    submit(&mut v, "query");
    focus_facets(&mut v);
    v.handle_key(key(KeyCode::Char('l'))); // rust chip
    v.handle_key(key(KeyCode::Enter)); // commit
    // A late batch lands: two more rust files in a second repo.
    v.update(&Action::GlobalSearchDelta {
        hits: vec![
            SearchHit::plain(
                "other/repo",
                "src/new.rs",
                7,
                vec![(7, "let query = 1;".to_string())],
                1,
                String::new(),
            ),
            SearchHit::plain(
                "other/repo",
                "src/aux.rs",
                9,
                vec![(9, "let query = 2;".to_string())],
                1,
                String::new(),
            ),
        ],
    });
    // The facet applies to the growing set…
    assert_eq!(v.visible().len(), 5, "new rust hits pass the facet");
    // …and the chips re-count over everything accumulated.
    let chips = v.facets();
    let rust = chips
        .iter()
        .find(|c| c.id.kind == facets::FacetKind::Lang && c.id.name == "rust")
        .expect("rust chip");
    assert_eq!(rust.count, 5);
    // The cursor stayed on a real chip.
    assert!(v.facet_cursor < chips.len());
}

#[test]
fn facet_composes_with_slash_filter() {
    let mut v = view();
    submit(&mut v, "query");
    focus_facets(&mut v);
    v.handle_key(key(KeyCode::Char('l'))); // rust chip
    v.handle_key(key(KeyCode::Enter)); // commit the rust facet
    // `/` then narrows inside the facet's set.
    v.handle_key(key(KeyCode::Tab)); // → results
    assert_eq!(v.focus, Focus::Results);
    v.handle_key(key(KeyCode::Char('/')));
    for c in "terminal".chars() {
        v.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(v.visible().len(), 1);
    assert_eq!(v.visible()[0].path, "src/terminal.rs");
    // Esc clears the text filter first — the facet survives.
    v.handle_key(key(KeyCode::Esc));
    assert_eq!(v.visible().len(), 3, "facet still committed");
}

#[test]
fn esc_peels_filter_then_facet_then_closes() {
    let mut v = view();
    submit(&mut v, "query");
    focus_facets(&mut v);
    v.handle_key(key(KeyCode::Char('l')));
    v.handle_key(key(KeyCode::Enter)); // commit rust facet
    assert_eq!(
        v.handle_key(key(KeyCode::Esc)),
        Action::Noop,
        "first Esc clears the facet, not the view"
    );
    assert!(v.facet.is_none());
    assert_eq!(v.visible().len(), 4);
    assert_eq!(
        v.handle_key(key(KeyCode::Esc)),
        Action::CloseSearchView,
        "second Esc closes"
    );
}

#[test]
fn new_search_resets_the_facet() {
    let mut v = view();
    submit(&mut v, "query");
    focus_facets(&mut v);
    v.handle_key(key(KeyCode::Enter)); // commit the repo facet
    assert!(v.facet.is_some());
    // Back to the query field (facets → results → query), then a
    // fresh search replaces the set.
    v.handle_key(key(KeyCode::Tab));
    v.handle_key(key(KeyCode::Tab));
    submit(&mut v, "other");
    assert!(v.facet.is_none(), "a new search is a new facet set");
    assert_eq!(v.facet_cursor, 0);
}
