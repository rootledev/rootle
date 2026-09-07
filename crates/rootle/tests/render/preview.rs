use super::*;

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
        screen.contains("10 │ fn f10() {}"),
        "gutter with divider missing:\n{screen}"
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
