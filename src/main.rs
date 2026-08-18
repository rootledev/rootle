//! Terminal lifecycle + event loop. Panic hook restores the terminal
//! before printing (PLAN.md §9: no stray output outside the draw path).

use ghx::app::App;
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
    // Headless version print: the release pipeline's smoke check
    // (PLAN.md §13) runs `ghx --version` on the static binary.
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("ghx {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

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

    let result = run(&mut terminal);

    disable_raw_mode()?;
    // Restore the user's cursor shape — never leave it mutated (PLAN.md §5).
    stdout().execute(SetCursorStyle::DefaultUserShape)?;
    stdout().execute(LeaveAlternateScreen)?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = App::new();
    loop {
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
        // Cursor shape follows input mode (bar=INSERT, block=NORMAL);
        // hidden automatically when no input is focused.
        if let Some(style) = app.cursor_style() {
            stdout().execute(style)?;
        }

        if event::poll(Duration::from_millis(250))? {
            // Resize: ratatui resizes its buffers automatically; the
            // next draw rewrites every cell. No manual clear (blink).
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
