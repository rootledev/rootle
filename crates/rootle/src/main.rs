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
use rootle::cli::{Cli, ProviderCommand, RootCommand};
use rootle::config::Config;
use std::io::{self, stdout};
use std::time::Duration;

mod terminal;

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    terminal::install_panic_hook();
    let session = match rootle::diagnostics::Session::start(&cli) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("trace: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let result = rootle_trace::in_operation(rootle_trace::operation_id(), || execute(cli));
    let result = match session {
        Some(session) => session.finish(result),
        None => result,
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli) -> io::Result<()> {
    cli.validate_execution()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    if let Some(command) = &cli.command {
        return match command {
            RootCommand::Provider { command } => run_provider(command)
                .map_err(|error| io::Error::other(format!("rootle provider: {error}"))),
            RootCommand::Update { check } => rootle::selfupdate::update(*check)
                .map_err(|error| io::Error::other(format!("update: {error}"))),
            RootCommand::SelfUpdate { check } => rootle::selfupdate::self_update(*check)
                .map_err(|error| io::Error::other(format!("self-update: {error}"))),
        };
    }
    if cli.update {
        return rootle::selfupdate::update(cli.check)
            .map_err(|error| io::Error::other(format!("update: {error}")));
    }
    if cli.headless.is_some() {
        return rootle::headless::run_cli(&cli);
    }

    // TUI colors carry meaning; only noninteractive CLI output honors NO_COLOR.
    ratatui::crossterm::style::Colored::set_ansi_color_disabled(false);
    let mut restore = terminal::Restore::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let result = run(&mut terminal, cli);
    let restored = restore.restore();
    let note = result?;
    restored?;
    if let Some(note) = note {
        println!("{note}");
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
    let (config, config_warning) = match &cli.config {
        Some(path) => Config::load_from(path),
        None => Config::load(),
    };
    let theme = cli.resolve_theme(&config);
    let (tx, rx) = rootle::event::channel();
    let mut app = App::new(tx, config, theme);
    app.config_warning(config_warning);
    // `rootle owner/repo`: skip search, go straight to browsing.
    // `owner/repo@ref` (plans/0016 M1a): open AT the revision — the ref
    // lands first, so the tree spawn below reads it.
    if let Some((owner, name, ref_)) = cli.repo_parts() {
        if let Some(r) = ref_ {
            app.handle_action(rootle::action::Action::RefsCommit(r));
        }
        app.handle_action(rootle::action::Action::RepoSelected { owner, name });
    }
    app.record_trace_state("startup");
    let mut last_cursor_style: Option<SetCursorStyle> = None;
    loop {
        if terminal::panicked() {
            return Err(io::Error::other(
                "worker thread panicked; terminal restored",
            ));
        }
        // Drain worker outcomes before drawing so a completed fetch
        // renders on this frame, not the next.
        while let Ok(event) = rx.try_recv() {
            app.handle_app_event(event);
        }
        if let Some(failure) = rootle_trace::take_failure() {
            app.report_trace_failure(failure);
        }
        // Full clear must precede the draw: Terminal::clear resets the
        // diff buffers, so the next draw re-renders every cell. Clearing
        // after a draw would leave the screen blank until the next event.
        if app.force_redraw {
            app.force_redraw = false;
            terminal::record_state("full_redraw_requested", true);
            terminal.clear()?;
        }
        terminal.draw(|frame| rootle::diagnostics::draw(&mut app, frame))?;
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
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Resize(columns, rows) => {
                    rootle_trace::record_with(
                        rootle_trace::EventKind::Resize,
                        || serde_json::json!({"columns":columns,"rows":rows,"source":"terminal"}),
                    );
                }
                ignored => {
                    // Keep existing dispatch behavior; diagnostics explain ignored input.
                    rootle_trace::record_with(rootle_trace::EventKind::Input, || match ignored {
                        Event::Paste(text) => serde_json::json!({
                            "source":"terminal", "kind":"paste", "handled":false,
                            "bytes":text.len(),
                            "text":rootle_trace::capture_content().then_some(text.as_str()),
                        }),
                        Event::FocusGained => {
                            serde_json::json!({"kind":"focus_gained","handled":false})
                        }
                        Event::FocusLost => {
                            serde_json::json!({"kind":"focus_lost","handled":false})
                        }
                        Event::Mouse(_) => serde_json::json!({"kind":"mouse","handled":false}),
                        Event::Key(_) | Event::Resize(..) => unreachable!(),
                    });
                }
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
            rootle_trace::record_with(rootle_trace::EventKind::State, || {
                serde_json::json!({"scope":"terminal","phase":"exit_requested",
                    "app_quit":app.should_quit,
                    "termination_signal":terminated_flag().load(std::sync::atomic::Ordering::Relaxed)})
            });
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
    terminal::record_state("suspended", true);
    let started = rootle_trace::enabled().then(std::time::Instant::now);
    rootle_trace::record_with(
        rootle_trace::EventKind::ExternalCommand,
        || serde_json::json!({"operation":"editor","phase":"started","argument_count":job.args.len()}),
    );

    let status = std::process::Command::new(&job.program)
        .args(&job.args)
        .status();
    rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
        serde_json::json!({
            "operation":"editor", "phase":"finished",
            "success":status.as_ref().is_ok_and(|status| status.success()),
            "exit_code":status.as_ref().ok().and_then(|status| status.code()),
            "error_kind":status.as_ref().err().map(|error|format!("{:?}",error.kind())),
            "duration_us":started.map(|started|started.elapsed().as_micros()),
        })
    });

    // Resume: raw mode + alternate screen again, then a full clear —
    // the editor scribbled on the screen, so the diff is unusable.
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    terminal.clear()?;
    terminal::record_state("resumed", true);

    // Drain input queued while suspended (editor residue, resizes).
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }
    Ok(())
}

/// Dispatch the provider subcommand tree (plans/0010 M3).
fn run_provider(cmd: &ProviderCommand) -> Result<(), rootle_manager::ManagerError> {
    rootle_trace::record_with(
        rootle_trace::EventKind::ExternalCommand,
        || serde_json::json!({"operation":"provider_command","phase":"started"}),
    );
    let result = execute_provider(cmd);
    rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
        let error_kind = result.as_ref().err().map(|error| match error {
            rootle_manager::ManagerError::User(_) => "user",
            rootle_manager::ManagerError::Network(_) => "network",
            rootle_manager::ManagerError::Io(_) => "io",
        });
        serde_json::json!({"operation":"provider_command","phase":"finished",
            "success":result.is_ok(),"error_kind":error_kind})
    });
    result
}

fn execute_provider(cmd: &ProviderCommand) -> Result<(), rootle_manager::ManagerError> {
    use rootle_manager::{Manager, ProviderReference};

    let manager = Manager::new()?;

    let result: std::result::Result<(), rootle_manager::ManagerError> = match cmd {
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
                let r = ProviderReference::parse(ref_)?;
                if *pin && r.tag.is_none() {
                    // Pin to the latest at install time.
                    eprintln!("--pin without @tag: pinning to the latest release");
                }
                manager.install(&r, *force).map(|_| ())
            }
        }
        ProviderCommand::List { json } => {
            let installed = rootle::provider::bookkeeping::list_installed(&manager);
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
                let ui = rootle_manager::progress::ProgressOutput::new();
                if installed.is_empty() {
                    ui.empty_hint();
                    return Ok(());
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
                return Err(rootle_manager::ManagerError::User(
                    "specify a provider name or --all".into(),
                ));
            }
            manager.upgrade(target, *dry_run, *force)
        }
        ProviderCommand::Pin { name, tag } => manager.pin(name, tag.clone()),
        ProviderCommand::Unpin { name } => manager.unpin(name),
        ProviderCommand::Remove { name } => manager.remove(name),
        ProviderCommand::Use { name, extra } => {
            rootle::provider::bookkeeping::activate(&manager, name, extra)
        }
    };

    result
}
