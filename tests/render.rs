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
    let mut app = App::new();
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
    let mut app = App::new();
    let _ = render(&mut app, 100, 30); // popup open

    // Esc twice: INSERT → NORMAL → close popup.
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc));
    let after = render(&mut app, 100, 30);
    let screen = after.join("\n");

    assert!(!screen.contains("search github"), "popup residue after close");
    assert!(screen.contains("BROWSE"), "should return to BROWSE");

    // The area where the popup was must show panes again, not blanks.
    let middle = &after[15];
    assert!(middle.contains('│') || middle.contains('╮') || middle.contains('╯'),
        "middle row lost pane borders after popup close");
}

#[test]
fn resize_keeps_modeline_on_last_row() {
    let mut app = App::new();
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc)); // close popup
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
    let mut app = App::new();
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc)); // close popup → repos pane
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
    let mut app = App::new();
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc)); // close popup

    // Drill into a repo: focus is now on the repo's root dir column.
    app.handle_key(key(KeyCode::Char('l')));
    let rows = render(&mut app, 100, 30);
    assert!(rows.join("\n").contains("Cargo.toml"), "should see repo root");

    // h: focus moves left into the repos column; j selects tokio-rs? —
    // first h reaches repos pane, then j moves to ratatui-website, and
    // the child column must cascade to the new repo's root.
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_key(key(KeyCode::Char('j')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("ratatui-website"),
        "child column should cascade to the newly selected repo"
    );
    // h again: focus reaches the orgs column → folds to a single pane.
    // j picks tokio-rs; the repos level cascades internally but stays
    // hidden while folded.
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_key(key(KeyCode::Char('j')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(rows[0].ends_with('╮'), "orgs should fold to full width");
    assert!(!screen.contains("axum/"), "folded view hides the repos column");

    // l unfolds: repos column shows tokio-rs repos, cascaded from the
    // new org selection — no stale ratatui entries.
    app.handle_key(key(KeyCode::Char('l')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("axum/"), "org switch should cascade repos");
    assert!(!screen.contains("comfy-table"), "stale child column leaked");
}

#[test]
fn org_level_folds_to_single_pane() {
    let mut app = App::new();
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc)); // close popup

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
    assert!(top.ends_with('╮'), "folded pane should reach the right edge");
}

#[test]
fn popup_results_support_local_slash_filter() {
    let mut app = App::new();
    // Popup opens on launch; submit empty query → all mock results.
    app.handle_key(key(KeyCode::Enter));

    // `/` in results → SEARCH chip, incremental local filter.
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Char('t')));
    app.handle_key(key(KeyCode::Char('o')));
    let rows = render(&mut app, 100, 30);
    let screen = rows.join("\n");
    assert!(screen.contains("SEARCH"), "filtering should show SEARCH chip");
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
