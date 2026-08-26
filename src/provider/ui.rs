//! CLI output aesthetics for the provider manager (plans/0010):
//! the uv/mise grammar — bold-cyan in-progress verbs that flip to
//! bold-green on completion, a braille spinner during downloads, dim
//! step summaries with bold counts and timings. Std only: raw ANSI,
//! `IsTerminal` gating, `NO_COLOR` respected.

use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct Ui {
    color: bool,
    stderr_tty: bool,
}

impl Ui {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Ui {
        let color = std::env::var("NO_COLOR").is_err() && std::io::stdout().is_terminal();
        Ui {
            color,
            stderr_tty: std::io::stderr().is_terminal(),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    /// `⠋ Resolving rootledev/rootle-gitlab…` — the in-progress line,
    /// replaced by the done line on the next step call.
    pub fn step(&self, verb: &str, detail: &str) {
        let mut err = std::io::stderr().lock();
        if self.stderr_tty {
            // Clear the previous line.
            let _ = write!(err, "\r\x1b[2K");
        }
        let _ = writeln!(
            err,
            " {} {} {detail}…",
            self.paint(CYAN, "●"),
            self.paint(&format!("{BOLD}{CYAN}"), verb),
        );
        let _ = err.flush();
    }

    /// `✓ Resolved rootledev/rootle-gitlab` — the completed step.
    /// Replaces the in-progress line (same position, same verb, now
    /// green, past tense implied by the check).
    pub fn done(&self, verb: &str, detail: &str) {
        let mut err = std::io::stderr().lock();
        if self.stderr_tty {
            let _ = write!(err, "\r\x1b[2K");
        }
        let _ = writeln!(
            err,
            " {} {} {detail}",
            self.paint(GREEN, "✓"),
            self.paint(&format!("{BOLD}{GREEN}"), verb),
        );
        let _ = err.flush();
    }

    /// The dim summary line: `Installed gitlab v0.1.0 in 2.3s`.
    pub fn summary(&self, verb: &str, subject: &str, detail: &str, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        let timing = if secs >= 1.0 {
            format!(" in {secs:.1}s")
        } else if secs * 1000.0 >= 100.0 {
            format!(" in {:.0}ms", secs * 1000.0)
        } else {
            String::new() // instant — no timing
        };
        let mut err = std::io::stderr().lock();
        let mut parts: Vec<String> = vec![
            self.paint(GREEN, "✓"),
            self.paint(BOLD, verb),
            self.paint(BOLD, subject),
        ];
        if !detail.is_empty() {
            parts.push(self.paint(DIM, detail));
        }
        if !timing.is_empty() {
            parts.push(self.paint(DIM, timing.trim_start()));
        }
        let _ = writeln!(err, "{}", parts.join(" "));
        let _ = err.flush();
    }

    /// An info line (trust notice, next-step hint).
    pub fn note(&self, text: &str) {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, " {} {}", self.paint(DIM, "▸"), self.paint(DIM, text));
        let _ = err.flush();
    }

    /// Start a braille spinner on a background thread; returns a guard
    /// whose Drop stops it and clears the line. The label is the
    /// in-progress message.
    pub fn spinner(&self, label: &str) -> SpinnerGuard {
        let stop = Arc::new(AtomicBool::new(false));
        let frame = Arc::new(AtomicUsize::new(0));
        let color = self.color;
        let label_owned = label.to_string();
        let handle = if self.stderr_tty {
            let stop = Arc::clone(&stop);
            let frame = Arc::clone(&frame);
            Some(std::thread::spawn(move || {
                let mut err = std::io::stderr().lock();
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let i = frame.load(Ordering::Relaxed) % SPINNER.len();
                    let sp = SPINNER[i];
                    let _ = write!(err, "\r\x1b[2K");
                    if color {
                        let _ = write!(err, "{CYAN}{sp}{RESET} {DIM}{label_owned}…{RESET}");
                    } else {
                        let _ = write!(err, "{sp} {label_owned}…");
                    }
                    let _ = err.flush();
                    std::thread::sleep(Duration::from_millis(90));
                    frame.fetch_add(1, Ordering::Relaxed);
                }
                let _ = write!(err, "\r\x1b[2K");
                let _ = err.flush();
            }))
        } else {
            // Non-TTY: print the label once, no animation.
            eprintln!("  {label_owned}…");
            None
        };
        SpinnerGuard { stop, handle }
    }

    /// A list row: `  gitlab  v0.1.0  rootledev/rootle-gitlab  ← ACTIVE`.
    pub fn row(&self, name: &str, version: &str, source: &str, active: bool, pinned: bool) {
        let mut err = std::io::stderr().lock();
        let pin = if pinned {
            format!(" {}", self.paint(YELLOW, "📌"))
        } else {
            String::new()
        };
        let marker = if active {
            format!(" {}", self.paint(&format!("{BOLD}{GREEN}"), "← active"))
        } else {
            String::new()
        };
        let _ = writeln!(
            err,
            "  {} {} {}{}{}",
            self.paint(BOLD, name),
            self.paint(GREEN, version),
            self.paint(DIM, &format!("from {source}")),
            pin,
            marker,
        );
        let _ = err.flush();
    }

    pub fn empty_hint(&self) {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(
            err,
            "  {} no providers installed — {}",
            self.paint(DIM, "·"),
            self.paint(&format!("{BOLD}{CYAN}"), "rootle provider install gitlab"),
        );
        let _ = err.flush();
    }
}

/// Stops the spinner thread on Drop; clears the line.
pub struct SpinnerGuard {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for SpinnerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// A wall-clock helper for the summary timing.
pub struct Timer {
    start: Instant,
}

impl Timer {
    pub fn start() -> Timer {
        Timer {
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}
