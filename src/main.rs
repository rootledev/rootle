//! Terminal lifecycle + event loop. Panic hook restores the terminal
//! before printing (PLAN.md §9: no stray output outside the draw path).

use clap::Parser;
use ratatui::crossterm::{
    ExecutableCommand,
    cursor::SetCursorStyle,
    event::{self, Event},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use rootle::app::App;
use rootle::cli::{Cli, ProviderCommand};
use rootle::config::Config;
use rootle::theme::{BorderShape, Theme};
use std::io::{self, stdout};
use std::time::Duration;

fn main() -> io::Result<()> {
    // clap handles --version/-V (release pipeline smoke check) and --help.
    let cli = Cli::parse();

    // Provider manager (plans/0010 M3): subcommands run and exit —
    // the TUI never starts.
    if let Some(cmd) = &cli.provider {
        run_provider(cmd);
        return Ok(());
    }

    // Self-update (plans/0017 M2, 0018 M1): same run-and-exit shape —
    // the stage UI renders on stderr, `Some(line)` is stdout's share.
    if cli.update {
        match rootle::update::update(cli.check) {
            Ok(Some(line)) => println!("{line}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("update: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // A full-screen TUI's colors are semantic (mode chips, dirs vs
    // files), not decoration — ignore NO_COLOR like vim/helix do.
    ratatui::crossterm::style::Colored::set_ansi_color_disabled(false);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // On panic: restore the terminal first, then let the hook print.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = stdout().execute(SetCursorStyle::DefaultUserShape);
        let _ = stdout().execute(LeaveAlternateScreen);
        default_hook(info);
    }));

    let result = run(&mut terminal, cli);

    disable_raw_mode()?;
    // Restore the user's cursor shape — never leave it mutated (PLAN.md §5).
    stdout().execute(SetCursorStyle::DefaultUserShape)?;
    stdout().execute(LeaveAlternateScreen)?;
    // 0018 M3: the restart trace lands on the real screen, after the
    // alternate screen is gone.
    match result {
        Ok(Some(line)) => println!("{line}"),
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

/// SIGTERM/SIGINT set this; the poll loop exits through the normal
/// cleanup path (terminal restore + App drop kills provider children).
fn terminated_flag() -> &'static std::sync::Arc<std::sync::atomic::AtomicBool> {
    static FLAG: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> =
        std::sync::OnceLock::new();
    FLAG.get_or_init(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cli: Cli,
) -> io::Result<Option<String>> {
    {
        use signal_hook::consts::{SIGINT, SIGTERM};
        let _ = signal_hook::flag::register(SIGTERM, terminated_flag().clone());
        let _ = signal_hook::flag::register(SIGINT, terminated_flag().clone());
    }
    let config = match &cli.config {
        Some(path) => Config::load_from(path),
        None => Config::load(),
    };
    let theme = Theme::load(cli.theme.as_deref().unwrap_or(&config.theme.name))
        .with_border(BorderShape::parse(&config.ui.border).unwrap_or_default())
        .with_nerd_font(config.ui.nerd_font)
        .with_separator(&config.ui.separator);
    let (tx, rx) = rootle::event::channel();
    let mut app = App::new(tx, config, theme);
    // `rootle owner/repo`: skip search, go straight to browsing.
    // `owner/repo@ref` (plans/0016 M1a): open AT the revision — the ref
    // lands first, so the tree spawn below reads it.
    if let Some((owner, name, ref_)) = cli.repo_parts() {
        if let Some(r) = ref_ {
            app.handle_action(rootle::action::Action::RefsCommit(r));
        }
        app.handle_action(rootle::action::Action::RepoSelected { owner, name });
    }
    let mut last_cursor_style: Option<SetCursorStyle> = None;
    loop {
        // Drain worker outcomes before drawing so a completed fetch
        // renders on this frame, not the next.
        while let Ok(event) = rx.try_recv() {
            app.handle_app_event(event);
        }
        // Full clear must precede the draw: Terminal::clear resets the
        // diff buffers, so the next draw re-renders every cell. Clearing
        // after a draw would leave the screen blank until the next event.
        if app.force_redraw {
            app.force_redraw = false;
            terminal.clear()?;
        }
        terminal.draw(|frame| {
            let area = frame.area();
            app.render(frame, area);
        })?;
        // Cursor shape follows input mode (bar=INSERT, block=NORMAL).
        // Emit only on CHANGE — repeating it every frame spams the
        // stream and races ratatui's own hide/show bookkeeping.
        let style = app.cursor_style();
        let changed = !matches!(
            (style, last_cursor_style),
            (None, None)
                | (
                    Some(SetCursorStyle::SteadyBar),
                    Some(SetCursorStyle::SteadyBar)
                )
                | (
                    Some(SetCursorStyle::SteadyBlock),
                    Some(SetCursorStyle::SteadyBlock)
                )
                | (
                    Some(SetCursorStyle::DefaultUserShape),
                    Some(SetCursorStyle::DefaultUserShape)
                )
        );
        if changed {
            if let Some(style) = style {
                stdout().execute(style)?;
            }
            last_cursor_style = style;
        }

        if event::poll(Duration::from_millis(250))? {
            // Resize: ratatui resizes its buffers automatically; the
            // next draw rewrites every cell. No manual clear (blink).
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        // Yank: write to the clipboard outside the draw path.
        if let Some(text) = app.take_clipboard() {
            rootle::clipboard::copy(&text);
        }

        // Editor: suspend the terminal, run the editor to completion,
        // resume with a full redraw (the one legitimate clear).
        if let Some(job) = app.take_editor_job() {
            run_editor(terminal, job)?;
            last_cursor_style = None; // editor reset the shape
        }

        if app.should_quit || terminated_flag().load(std::sync::atomic::Ordering::Relaxed) {
            // 0018 M3: compare the on-disk binary once, post-update
            // sessions only — main prints it after terminal restore.
            return Ok(app.update_exit_note());
        }
    }
}

fn run_editor(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    job: rootle::editor::EditorJob,
) -> io::Result<()> {
    // Suspend: leave the alternate screen, raw mode off, cursor normal.
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    stdout().execute(SetCursorStyle::DefaultUserShape)?;

    let status = std::process::Command::new(&job.program)
        .args(&job.args)
        .status();
    rootle::app::trace(&format!(
        "editor {} exited: {}",
        job.program,
        status
            .map(|s| s.to_string())
            .unwrap_or_else(|e| e.to_string())
    ));

    // Resume: raw mode + alternate screen again, then a full clear —
    // the editor scribbled on the screen, so the diff is unusable.
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    terminal.clear()?;

    // Drain input queued while suspended (editor residue, resizes).
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }
    Ok(())
}

/// Dispatch the provider subcommand tree (plans/0010 M3).
fn run_provider(cmd: &ProviderCommand) {
    use rootle::provider::manager::{Manager, Ref};

    let manager = match Manager::new() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("rootle provider: {e}");
            std::process::exit(1);
        }
    };

    let result: std::result::Result<(), rootle::provider::manager::ManagerError> = match cmd {
        ProviderCommand::Install {
            ref_,
            pin,
            force,
            path,
        } => {
            if let Some(path) = path {
                // --path: local binary install (gh's `gh extension
                // install .` model); ref_ carries the name.
                manager.install_path(ref_, path).map(|_| ())
            } else {
                let mut r = match Ref::parse(ref_) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                };
                if *pin && r.tag.is_none() {
                    // Pin to the latest at install time.
                    eprintln!("--pin without @tag: pinning to the latest release");
                }
                r.tag = r.tag.take(); // no-op, keeps the Option alive
                manager.install(&r, *force).map(|_| ())
            }
        }
        ProviderCommand::List { json } => {
            let installed = manager.list();
            if *json {
                let rows: Vec<serde_json::Value> = installed
                    .iter()
                    .map(|i| {
                        serde_json::json!({
                            "name": i.receipt.name,
                            "version": i.receipt.tag,
                            "pinned": i.receipt.pinned,
                            "source": i.receipt.source,
                            "active": i.active,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows).unwrap());
            } else {
                let ui = rootle::provider::ui::Ui::new();
                if installed.is_empty() {
                    ui.empty_hint();
                    return;
                }
                for i in &installed {
                    ui.row(
                        &i.receipt.name,
                        &i.receipt.tag,
                        &i.receipt.source,
                        i.active,
                        i.receipt.pinned,
                    );
                }
            }
            Ok(())
        }
        ProviderCommand::Update { name } => match manager.update(name.as_deref()) {
            Ok(stale) => {
                if stale.is_empty() {
                    println!("all providers current");
                } else {
                    for (name, from, to) in stale {
                        println!(
                            "  {name}: {from} → {to} (upgrade with `rootle provider upgrade {name}`)"
                        );
                    }
                }
                Ok(())
            }
            Err(e) => Err(e),
        },
        ProviderCommand::Upgrade {
            name,
            all,
            dry_run,
            force,
        } => {
            let target = if *all { None } else { name.as_deref() };
            if !*all && name.is_none() {
                eprintln!("specify a provider name or --all");
                std::process::exit(1);
            }
            manager.upgrade(target, *dry_run, *force)
        }
        ProviderCommand::Pin { name, tag } => manager.pin(name, tag.clone()),
        ProviderCommand::Unpin { name } => manager.unpin(name),
        ProviderCommand::Remove { name } => manager.remove(name),
        ProviderCommand::Use { name, extra } => manager.activate(name, extra),
    };

    if let Err(e) = result {
        eprintln!("rootle provider: {e}");
        std::process::exit(1);
    }
}
