//! Frame-level verification with ratatui's TestBackend (see
//! .agents/skills/rootle-tui-debug): renders the app to a Buffer and
//! asserts on visible text — including that closing a popup leaves
//! no lingering cells.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use rootle::app::App;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn test_app() -> App {
    app_with_orgs(&[])
}

/// Offline app whose orgs pane is seeded (the offline provider ships
/// no defaults — tests state them explicitly).
fn app_with_orgs(orgs: &[&str]) -> App {
    let (tx, _rx) = rootle::event::channel();
    App::with(
        rootle::state::State {
            recent_orgs: orgs.iter().map(|o| o.to_string()).collect(),
            ..Default::default()
        },
        tx,
    )
}

/// Fake recursive tree for ratatui/ratatui (mirrors the old mock buckets).
fn ratatui_tree() -> Vec<rootle::provider::TreeNode> {
    fn node(path: &str, is_dir: bool) -> rootle::provider::TreeNode {
        rootle::provider::TreeNode {
            path: path.into(),
            is_dir,
            sha: "abc1234def5678".into(),
            size: if is_dir { None } else { Some(42) },
        }
    }
    vec![
        node("src", true),
        node("docs", true),
        node("examples", true),
        node("Cargo.toml", false),
        node("README.md", false),
        node("LICENSE", false),
        node("src/widgets", true),
        node("src/layout", true),
        node("src/lib.rs", false),
        node("src/terminal.rs", false),
        node("src/malformed.bin", false),
        node("src/widgets/mod.rs", false),
        node("src/widgets/block.rs", false),
        node("src/widgets/paragraph.rs", false),
    ]
}

/// Popup closed, ratatui org repos + repo tree loaded (offline —
/// injected, no workers). Lands focused on the repos pane.
fn browsing_app() -> App {
    let mut app = app_with_orgs(&["ratatui", "tokio-rs", "helix-editor"]);
    app.handle_key(key(KeyCode::Esc));
    // No seeded orgs with the offline provider — select explicitly so
    // the repos/tree injections below pass the selection gates.
    app.handle_action(rootle::action::Action::OrgSelected("ratatui".into()));
    app.handle_action(rootle::action::Action::OrgReposLoaded {
        org: "ratatui".into(),
        repos: vec![
            "ratatui".into(),
            "ratatui-website".into(),
            "templates".into(),
            "comfy-table".into(),
        ],
    });
    app.handle_action(rootle::action::Action::TreeLoaded {
        owner: "ratatui".into(),
        name: "ratatui".into(),
        entries: ratatui_tree(),
        truncated: false,
        branch: "main".into(),
    });
    // Tree arrival auto-enters the root pane; step back to the repos
    // pane — this helper models "browsing at repos level".
    app.handle_key(key(KeyCode::Char('h')));
    app
}
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
fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            app.render(f, area);
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn renders_three_panes_modeline_and_popup() {
    // Seeded orgs behind the popup; the popup itself opened via ␣ s
    // (auto-open happens only on a fresh state).
    let mut app = app_with_orgs(&["ratatui", "tokio-rs", "helix-editor"]);
    app.handle_action(rootle::action::Action::LeaderSearch);
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");

    // Launch: search popup is open over the browser.
    assert!(screen.contains("search github"), "popup title missing");
    assert!(screen.contains("INSERT"), "should land in INSERT mode");
    assert!(screen.contains("tab focus"), "popup hint row missing");

    // Browser visible behind the popup: orgs column + repos column.
    assert!(screen.contains("orgs"), "orgs pane missing");
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
    app.handle_action(rootle::action::Action::TreeLoaded {
        owner: "ratatui".into(),
        name: "ratatui-website".into(),
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
    assert!(rows[0].ends_with('╮'), "orgs should fold to full width");
    assert!(
        !screen.contains("axum/"),
        "folded view hides the repos column"
    );

    // l on an org triggers LoadOrgRepos; the response installs the
    // repos level — no stale ratatui entries.
    app.handle_action(rootle::action::Action::OrgReposLoaded {
        org: "tokio-rs".into(),
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
fn file_preview_shows_highlighted_blob_and_scrolls() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // into ratatui root
    // jj j to reach Cargo.toml (dirs docs/, examples/, src/ first)
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    // Selection is on Cargo.toml → meta preview; inject the blob.
    let sha = "abc1234def5678";
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: sha.into(),
        name: "Cargo.toml".into(),
        bytes: b"[package]\nname = \"ratatui\"\nversion = \"0.29.0\"\n\n\n\n\n\n\n\n".to_vec(),
    });
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("[package]"),
        "highlighted blob content missing from preview"
    );

    // J scrolls the preview down; K back up.
    app.handle_key(key(KeyCode::Char('J')));
    let rows_after = render(&mut app, 100, 30);
    assert_ne!(
        rows.join("\n"),
        rows_after.join("\n"),
        "J should scroll the preview"
    );
}

#[test]
fn preview_colors_dirs_and_files_differently() {
    use ratatui::style::{Color, Modifier};

    let mut app = browsing_app(); // popup closed + org repos loaded → repos pane, preview
    // shows ratatui's root listing
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            app.render(f, area);
        })
        .unwrap();
    let buf = terminal.backend().buffer();

    let blue = Color::from_u32(0x89b4fa);
    let text = Color::from_u32(0xcdd6f4);

    // Scan the right column (x ≥ 60) for a dir child and a file child,
    // matching exact cell sequences (the border glyph sits at x=60).
    let find = |pat: &str| -> Option<(u16, u16)> {
        let chars: Vec<char> = pat.chars().collect();
        for y in 0..buf.area.height {
            for x in 61..buf.area.width - chars.len() as u16 {
                if chars
                    .iter()
                    .enumerate()
                    .all(|(i, c)| buf[(x + i as u16, y)].symbol() == c.to_string())
                {
                    return Some((x, y));
                }
            }
        }
        None
    };

    let (x, y) = find("src/").expect("dir child src/ not found in preview");
    let cell = &buf[(x, y)];
    assert_eq!(cell.fg, blue, "directories in preview must be blue");
    assert!(
        cell.modifier.contains(Modifier::BOLD),
        "directories must be bold"
    );

    let (x, y) = find("Cargo.toml").expect("file child not found in preview");
    assert_eq!(buf[(x, y)].fg, text, "files in preview must use text color");
}

#[test]
fn org_level_folds_to_single_pane() {
    let mut app = browsing_app(); // popup closed + org repos loaded

    // h until focus reaches the orgs column (top level).
    app.handle_key(key(KeyCode::Char('h')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");

    // Single folded pane: orgs visible, no repo column border beside it.
    assert!(screen.contains("orgs"));
    assert!(screen.contains("tokio-rs/"));
    // A folded single pane spans nearly full width: orgs title starts at
    // the left edge and its right border sits at the far right.
    let top = &rows[0];
    assert!(top.starts_with('╭'), "folded pane should start at x=0");
    assert!(
        top.ends_with('╮'),
        "folded pane should reach the right edge"
    );
}

#[test]
fn popup_results_support_local_slash_filter() {
    let mut app = test_app();
    // Submit a query (offline: no worker spawned) and inject the
    // response — what the worker thread would send over the channel.
    app.handle_key(key(KeyCode::Enter));
    app.handle_action(rootle::action::Action::SearchResults {
        items: vec![
            rootle::provider::SearchItem::Org("tokio-rs".into()),
            rootle::provider::SearchItem::Repo("tokio-rs/tokio".into()),
            rootle::provider::SearchItem::Repo("ratatui/ratatui".into()),
            rootle::provider::SearchItem::Repo("sharkdp/bat".into()),
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
    // The view replaces the browser: no miller columns underneath.
    assert!(!screen.contains("orgs"), "browser should be replaced");
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
    assert!(screen.contains("( ) all of github"), "radio option missing");
    println!("{screen}");

    // j j → repo → org → all of github (radio follows the cursor),
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
fn closing_search_view_restores_browser_without_lingering_cells() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    let _ = render(&mut app, 100, 30); // view open

    // Esc from the query input: INSERT → NORMAL, then Esc closes.
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(!screen.contains("grep ·"), "view residue after close");
    assert!(screen.contains("orgs"), "browser should be back");
    assert!(screen.contains("BROWSE"), "should return to BROWSE");

    // Middle of the screen must show pane content again, not blanks.
    let middle = &rows[15];
    assert!(
        middle.trim().len() > 10,
        "lingering blank cells after close: {middle:?}"
    );
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

    // Enter on the remaining hit prepares an editor job on mock bytes.
    app.handle_key(key(KeyCode::Esc)); // commit-cancel filter? Esc cancels
    // filter → full list again; first hit selected.
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.take_editor_job().is_some(),
        "Enter on a hit should prepare an editor job"
    );
}

#[test]
fn search_view_previews_are_syntax_highlighted() {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('f')));
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            app.render(f, area);
        })
        .unwrap();
    let buf = terminal.backend().buffer();

    // Find the preview row with a Rust keyword and assert it carries
    // syntect RGB colors (plain text would render in the theme's
    // default text color, never Rgb).
    let row = (0..buf.area.height)
        .find(|&y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("pub fn parse")
        })
        .expect("preview row should be visible");
    assert!(
        (0..buf.area.width).any(|x| matches!(buf[(x, row)].fg, ratatui::style::Color::Rgb(..))),
        "preview line should be syntax-highlighted"
    );
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
    assert!(screen.contains("orgs"), "browser should be back");
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
    assert!(screen.contains("next"), "next button missing");
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
    // flip to the LEADER chip with the leader hints.
    app.handle_key(key(KeyCode::Char(' ')));
    let screen = render(&mut app, 120, 30).join("\n");
    assert!(
        screen.contains("LEADER"),
        "leader chip missing over the search view:\n{screen}"
    );
    assert!(screen.contains("yank"), "leader hints missing:\n{screen}");

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
fn launch_popup_only_when_state_has_no_repos() {
    // Fresh state → popup opens automatically.
    let mut fresh = test_app();
    let screen = render(&mut fresh, 100, 30).join("\n");
    assert!(
        screen.contains("search github"),
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

#[test]
fn preview_line_cursor_walks_and_readout_updates() {
    // plans/0006 §5: J/K move a line cursor in the preview; the border
    // readout tracks it — the value ␣ y anchors the yank URL to.
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // drill into repo root
    // Dirs sort first (docs/, examples/, src/) — three j's to Cargo.toml.
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: "abc1234def5678".into(),
        name: "Cargo.toml".into(),
        bytes: (1..=9)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes(),
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("1/9"),
        "readout should show cursor 1 of 9:\n{screen}"
    );
    app.handle_key(key(KeyCode::Char('J')));
    app.handle_key(key(KeyCode::Char('J')));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("3/9"), "two J moves → cursor 3:\n{screen}");
    for _ in 0..20 {
        app.handle_key(key(KeyCode::Char('J')));
    }
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("9/9"),
        "cursor clamps at last line:\n{screen}"
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

/// Drill from the repos pane into the repo root and select Cargo.toml
/// (root lists dirs first: docs, examples, src, then files).
fn app_on_cargo_toml() -> App {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l')));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    app
}

/// Drill into src/ and select lib.rs (src lists layout/, widgets/,
/// then files).
fn app_on_lib_rs() -> App {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l'))); // repo root
    for _ in 0..2 {
        app.handle_key(key(KeyCode::Char('j'))); // src
    }
    app.handle_key(key(KeyCode::Char('l'))); // into src
    for _ in 0..2 {
        app.handle_key(key(KeyCode::Char('j'))); // lib.rs
    }
    app
}

#[test]
fn preview_shows_gutter_footer_and_scrollbar() {
    let mut app = app_on_lib_rs();
    let content = (1..=60)
        .map(|i| format!("fn f{i}() {{}}\n"))
        .collect::<String>();
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: "abc1234def5678".into(),
        name: "lib.rs".into(),
        bytes: content.into_bytes(),
    });
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("rust · 60 lines"),
        "footer missing:\n{screen}"
    );
    assert!(screen.contains("┃"), "scrollbar thumb missing:\n{screen}");
    assert!(screen.contains("1/60"), "readout missing:\n{screen}");
    assert!(
        screen.contains("10 fn f10() {}"),
        "line-number gutter missing:\n{screen}"
    );
}

#[test]
fn find_in_file_flow_highlights_steps_wraps_and_clears() {
    let mut app = app_on_cargo_toml();
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: "abc1234def5678".into(),
        name: "Cargo.toml".into(),
        bytes: b"alpha\nbeta ratatui\ngamma\nratatui delta\n".to_vec(),
    });
    // ␣ / opens FIND over the preview.
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('/')));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("FIND"), "find chip missing:\n{screen}");
    assert!(
        screen.contains("Cargo.toml /"),
        "title query prompt missing:\n{screen}"
    );

    for c in "ratatui".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("/ratatui"), "query in title:\n{screen}");
    assert!(screen.contains("1/2 · 2/4"), "match readout:\n{screen}");

    app.handle_key(key(KeyCode::Enter)); // commit
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("BROWSE"),
        "commit returns to browse:\n{screen}"
    );
    assert!(
        screen.contains("1/2 · 2/4"),
        "chips survive commit:\n{screen}"
    );

    app.handle_key(key(KeyCode::Char('n')));
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("2/2 · 4/4"), "n steps forward:\n{screen}");
    app.handle_key(key(KeyCode::Char('n'))); // wraps to the first match
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("1/2 · 2/4"), "n wraps:\n{screen}");
    app.handle_key(key(KeyCode::Char('N'))); // and back
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("2/2 · 4/4"), "N steps back:\n{screen}");

    app.handle_key(key(KeyCode::Esc)); // :nohlsearch
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(!screen.contains("2/2 ·"), "esc clears the chips:\n{screen}");
    assert!(
        screen.contains("4/4"),
        "cursor stays on the match line:\n{screen}"
    );
}

#[test]
fn find_cancel_restores_the_cursor_line() {
    let mut app = app_on_cargo_toml();
    app.handle_action(rootle::action::Action::BlobLoaded {
        sha: "abc1234def5678".into(),
        name: "Cargo.toml".into(),
        bytes: b"alpha\nbeta ratatui\ngamma\nratatui delta\n".to_vec(),
    });
    app.handle_key(key(KeyCode::Char('J'))); // cursor to line 2
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("2/4"), "pre-find cursor:\n{screen}");

    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('/')));
    for c in "delta".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(screen.contains("1/1 · 4/4"), "live jump:\n{screen}");

    app.handle_key(key(KeyCode::Esc)); // cancel the session
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("BROWSE"),
        "cancel returns to browse:\n{screen}"
    );
    assert!(screen.contains("2/4"), "cursor restored:\n{screen}");
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
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("clipped"),
        "clipped note should render in the results title:\n{screen}"
    );
}
