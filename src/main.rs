//! Terminal lifecycle + event loop. Panic hook restores the terminal
//! before printing (PLAN.md §9: no stray output outside the draw path).

use clap::Parser;
use ghx::app::App;
use ghx::cli::Cli;
use ghx::config::Config;
use ghx::theme::Theme;
use ratatui::crossterm::{
    cursor::SetCursorStyle,
    event::{self, Event},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, stdout};
use std::time::Duration;

fn main() -> io::Result<()> {
    // clap handles --version/-V (release pipeline smoke check) and --help.
    let cli = Cli::parse();

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
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, cli: Cli) -> io::Result<()> {
    let config = match &cli.config {
        Some(path) => Config::load_from(path),
        None => Config::load(),
    };
    let theme = Theme::load(cli.theme.as_deref().unwrap_or(&config.theme.name));
    let (tx, rx) = ghx::event::channel();
    let mut app = App::new(tx, config, theme);
    // `ghx owner/repo`: skip search, go straight to browsing.
    if let Some((owner, name)) = cli.repo_parts() {
        app.handle_action(ghx::action::Action::RepoSelected { owner, name });
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

        // Editor: suspend the terminal, run the editor to completion,
        // resume with a full redraw (the one legitimate clear).
        if let Some(job) = app.take_editor_job() {
            run_editor(terminal, job)?;
            last_cursor_style = None; // editor reset the shape
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn run_editor(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    job: ghx::editor::EditorJob,
) -> io::Result<()> {
    // Suspend: leave the alternate screen, raw mode off, cursor normal.
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    stdout().execute(SetCursorStyle::DefaultUserShape)?;

    let status = std::process::Command::new(&job.program)
        .args(&job.args)
        .status();
    ghx::app::trace(&format!(
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
