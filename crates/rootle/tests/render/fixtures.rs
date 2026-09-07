use super::*;

pub(super) fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

pub(super) fn test_app() -> App {
    app_with_orgs(&[])
}

/// Offline app whose orgs pane is seeded (the offline provider ships
/// no defaults — tests state them explicitly).
pub(super) fn app_with_orgs(orgs: &[&str]) -> App {
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
pub(super) fn ratatui_tree() -> Vec<rootle_provider::TreeNode> {
    fn node(path: &str, is_dir: bool) -> rootle_provider::TreeNode {
        rootle_provider::TreeNode {
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
pub(super) fn browsing_app() -> App {
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

pub(super) fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
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

/// Drill from the repos pane into the repo root and select Cargo.toml
/// (root lists dirs first: docs, examples, src, then files).
pub(super) fn app_on_cargo_toml() -> App {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char('l')));
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    app
}

/// Drill into src/ and select lib.rs (src lists layout/, widgets/,
/// then files).
pub(super) fn app_on_lib_rs() -> App {
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

/// Open the grep view over an injected single hit (offline, no workers).
pub(super) fn grep_view_on_hit(hit: rootle::components::global_search::SearchHit) -> App {
    let mut app = browsing_app();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    for c in "query".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_action(rootle::action::Action::GlobalSearchResults {
        hits: vec![hit],
        clipped: false,
        index: None,
        client_filtered: 0,
        unfiltered: vec![],
    });
    app
}
