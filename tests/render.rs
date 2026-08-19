//! Frame-level verification with ratatui's TestBackend (see
//! .agents/skills/ghx-tui-debug): renders the app to a Buffer and
//! asserts on visible text — including that closing a popup leaves
//! no lingering cells.

use ghx::app::App;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::Terminal;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn test_app() -> App {
    let (tx, _rx) = ghx::event::channel();
    App::with(ghx::state::State::default(), tx)
}

/// Fake recursive tree for ratatui/ratatui (mirrors the old mock buckets).
fn ratatui_tree() -> Vec<ghx::github::TreeNode> {
    fn node(path: &str, is_dir: bool) -> ghx::github::TreeNode {
        ghx::github::TreeNode {
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
    let mut app = test_app();
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc));
    app.handle_action(ghx::action::Action::OrgReposLoaded {
        org: "ratatui".into(),
        repos: vec![
            "ratatui".into(),
            "ratatui-website".into(),
            "templates".into(),
            "comfy-table".into(),
        ],
    });
    app.handle_action(ghx::action::Action::TreeLoaded {
        owner: "ratatui".into(),
        name: "ratatui".into(),
        entries: ratatui_tree(),
        truncated: false,
    });
    app
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
    let mut app = test_app();
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
    app.handle_action(ghx::action::Action::TreeLoaded {
        owner: "ratatui".into(),
        name: "ratatui-website".into(),
        entries: ratatui_tree(),
        truncated: false,
    });
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("Cargo.toml"),
        "child column should appear with the tree"
    );
    // h again: focus reaches the orgs column → folds to a single pane.
    // j picks tokio-rs. Org repos now arrive from the API — inject the
    // response (offline app never spawns workers).
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
    app.handle_action(ghx::action::Action::OrgReposLoaded {
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
    app.handle_action(ghx::action::Action::SearchResults {
        items: vec![
            ghx::github::SearchItem::Org("tokio-rs".into()),
            ghx::github::SearchItem::Repo("tokio-rs/tokio".into()),
            ghx::github::SearchItem::Repo("ratatui/ratatui".into()),
            ghx::github::SearchItem::Repo("sharkdp/bat".into()),
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
