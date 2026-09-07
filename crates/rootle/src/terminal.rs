//! Binary-owned terminal lifetime. Panic restoration never calls the logger
//! until raw mode and the alternate screen are gone, even during startup.

use ratatui::crossterm::{
    ExecutableCommand,
    cursor::SetCursorStyle,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io::{self, stdout};
use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static PANICKED: AtomicBool = AtomicBool::new(false);

pub fn panicked() -> bool {
    PANICKED.load(Ordering::Acquire)
}

pub struct Restore;

impl Restore {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        ACTIVE.store(true, Ordering::Release);
        let guard = Self;
        stdout().execute(EnterAlternateScreen)?;
        record_state("entered", true);
        Ok(guard)
    }

    pub fn restore(&mut self) -> io::Result<()> {
        let result = restore_terminal();
        record_state("restored", result.is_ok());
        result
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}

fn restore_terminal() -> io::Result<()> {
    if !ACTIVE.swap(false, Ordering::AcqRel) {
        return Ok(());
    }
    // Attempt every restoration step even when one fails.
    let raw = disable_raw_mode();
    let cursor = stdout()
        .execute(SetCursorStyle::DefaultUserShape)
        .map(|_| ());
    let screen = stdout().execute(LeaveAlternateScreen).map(|_| ());
    raw.and(cursor).and(screen)
}

pub fn record_state(phase: &'static str, success: bool) {
    rootle_trace::record_with(
        rootle_trace::EventKind::State,
        || serde_json::json!({"scope":"terminal", "phase":phase, "success":success}),
    );
}

pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        PANICKED.store(true, Ordering::Release);
        let _ = restore_terminal();
        let content = rootle_trace::capture_content();
        let message = content.then(|| info.to_string());
        let backtrace = content.then(|| std::backtrace::Backtrace::force_capture().to_string());
        rootle_trace::panic_record(
            info.location()
                .map(|location| (location.file(), location.line(), location.column())),
            message.as_deref(),
            backtrace.as_deref(),
        );
        default(info);
    }));
}
