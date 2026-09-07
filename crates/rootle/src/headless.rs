//! Headless driver (plans/0023 M1): scripted keys in, cell-grid
//! frames + state JSON out. Deterministic — no PTY, no raw mode, no
//! alternate screen. The TUI and this driver share one input path:
//! both feed `App::handle_key`, and both render through `App::render`
//! (here onto ratatui's `TestBackend` instead of a terminal).
//!
//! Script format, one step per line (`#` comments, blanks ignored):
//!
//! ```text
//! keys <text>   feed keys; token forms: <esc> <cr> <bs> <tab>
//!               <space> <up> <down> <left> <right>
//! settle [ms]   wait for all workers and their queued follow-ups
//!               (default 10000ms); timeout fails the run
//! wait <ms>     drain events for N ms — real providers reply on
//!               their own clock
//! frame         dump the cell grid
//! state         dump one JSON line (mode, overlays, context, …)
//! ```
//!
//! `keys` drains only what already arrived (pane moves, filters —
//! synchronous); a step that awaits a provider round-trip (tree
//! loads, searches) wants a following `settle`/`wait` before
//! `frame`/`state`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::io::Write;
use std::time::{Duration, Instant};

use crate::app::App;
use crate::event::AppRx;

mod frame;
pub use frame::buffer_text;

/// Default script viewport — `ROOTLE_HEADLESS_COLS` /
/// `ROOTLE_HEADLESS_ROWS` override (pane-layout stress needs sizes).
pub const DEFAULT_COLS: u16 = 100;
pub const DEFAULT_ROWS: u16 = 30;

/// `settle`'s default bound — a stalled provider must fail the run
/// explicitly (0020 §4's honesty rule applies to scripts) instead of
/// wedging the driver or half-loading a frame. `settle <ms>` tightens
/// (or widens) it per step.
const SETTLE_BOUND: Duration = Duration::from_secs(10);
/// Poll cadence while workers are outstanding. The ticket drop is
/// the real signal; this only paces the drain loop (and keeps an
/// empty-queue wait cheap).
const SETTLE_POLL: Duration = Duration::from_millis(5);

pub struct Headless {
    app: App,
    rx: AppRx,
    cols: u16,
    rows: u16,
    terminal: Terminal<TestBackend>,
    /// Editor invocations the TUI would suspend for; recorded, never
    /// run (no terminal to suspend).
    editor_jobs: Vec<String>,
    /// Yanks the main loop would write to the clipboard; recorded.
    yanks: Vec<String>,
}

impl Headless {
    pub fn new(app: App, rx: AppRx, cols: u16, rows: u16) -> Self {
        Headless {
            app,
            rx,
            cols,
            rows,
            terminal: Terminal::new(TestBackend::new(cols, rows)).expect("test backend"),
            editor_jobs: Vec::new(),
            yanks: Vec::new(),
        }
    }

    /// Drain queued worker outcomes — the TUI does this once per tick,
    /// before drawing. Returns whether anything arrived.
    fn drain(&mut self) -> bool {
        let mut drained = false;
        while let Ok(event) = self.rx.try_recv() {
            self.app.handle_app_event(event);
            drained = true;
        }
        if let Some(failure) = rootle_trace::take_failure() {
            self.app.report_trace_failure(failure);
        }
        self.collect_side_effects();
        drained
    }

    /// The main loop's out-of-draw side effects, recorded instead of
    /// executed: no clipboard to write, no editor to suspend into.
    /// Exception: `ROOTLE_CLIPBOARD=<path>` (the e2e/CI override) is a
    /// plain file write, not a terminal escape — honored for fidelity.
    fn collect_side_effects(&mut self) {
        if let Some(job) = self.app.take_editor_job() {
            rootle_trace::record_with(
                rootle_trace::EventKind::ExternalCommand,
                || serde_json::json!({"operation":"editor","phase":"recorded","executed":false,"argument_count":job.args.len()}),
            );
            let mut cmd = job.program;
            if !job.args.is_empty() {
                cmd.push(' ');
                cmd.push_str(&job.args.join(" "));
            }
            self.editor_jobs.push(cmd);
        }
        if let Some(text) = self.app.take_clipboard() {
            rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
                serde_json::json!({"operation":"clipboard","phase":"recorded","bytes":text.len(),
                    "executed":std::env::var_os("ROOTLE_CLIPBOARD").is_some()})
            });
            if std::env::var_os("ROOTLE_CLIPBOARD").is_some() {
                crate::clipboard::copy(&text);
            }
            self.yanks.push(text);
        }
    }

    /// Drain to real quiescence — no quiet-window guessing, no
    /// status-string reading. A worker's ticket outlives its last
    /// `send` (workers/tracker.rs), so a zero outstanding count means
    /// every finished worker's events are already queued; the drain
    /// that follows picks them up (and any cascade spawns they
    /// trigger keep the loop alive). Done = count zero AND a full
    /// drain that found nothing. Bounded: a stalled provider fails
    /// the run instead of wedging it.
    fn settle(&mut self, bound: Duration) -> std::io::Result<()> {
        let started = Instant::now();
        let workers = self.app.outstanding_workers();
        loop {
            if workers.count() == 0 {
                // Everything already sent is queued; drain until a
                // pass comes back empty (events drained here can
                // spawn follow-up workers and keep us waiting).
                if !self.drain() {
                    return Ok(());
                }
                continue;
            }
            if started.elapsed() >= bound {
                let outstanding = workers.count();
                rootle_trace::record_with(rootle_trace::EventKind::Error, || {
                    serde_json::json!({
                        "operation":"headless_settle",
                        "reason":"deadline",
                        "bound_ms":bound.as_millis(),
                        "outstanding":outstanding,
                    })
                });
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "settle timed out after {}ms — {outstanding} worker(s) still outstanding",
                        bound.as_millis(),
                    ),
                ));
            }
            self.drain();
            std::thread::sleep(SETTLE_POLL);
        }
    }

    /// Wall-clock drain for real providers (strop's `wait` bent the
    /// "zero timing" ideal the same way: data sources don't care
    /// about our determinism).
    fn wait(&mut self, ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < deadline {
            self.drain();
            std::thread::sleep(Duration::from_millis(10));
        }
        self.drain();
    }

    pub fn frame_string(&mut self) -> String {
        self.terminal
            .draw(|frame| crate::diagnostics::draw(&mut self.app, frame))
            .expect("draw");
        buffer_text(self.terminal.backend().buffer())
    }

    pub fn state_json(&self) -> String {
        let mut state = self.app.snapshot();
        state["editor_jobs"] = self.editor_jobs.clone().into();
        state["yanks"] = self.yanks.clone().into();
        state.to_string()
    }

    /// Interpret a script (module docs describe the language),
    /// writing frames/states to `out`. A `settle` that hits its bound
    /// stops the run: later frames would capture a half-loaded lie.
    pub fn run_script(&mut self, script: &str, out: &mut dyn Write) -> std::io::Result<()> {
        for line in script.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if self.app.should_quit {
                break; // nothing left to drive
            }
            if let Some(keys) = line.strip_prefix("keys ") {
                for key in parse_keys(keys) {
                    self.app.handle_key(key);
                }
                self.drain();
            } else if let Some(bound) = settle_bound(line) {
                self.settle(bound?)?;
            } else if let Some(ms) = line.strip_prefix("wait ") {
                self.wait(ms.trim().parse().unwrap_or(500));
            } else if line == "frame" {
                self.drain();
                let _ = writeln!(out, "─── frame {}×{}", self.cols, self.rows);
                let _ = write!(out, "{}", self.frame_string());
            } else if line == "state" {
                self.drain();
                let _ = writeln!(out, "─── state {}", self.state_json());
            }
        }
        Ok(())
    }
}

/// A malformed timeout must not silently skip the requested wait.
fn settle_bound(line: &str) -> Option<std::io::Result<Duration>> {
    if line == "settle" {
        return Some(Ok(SETTLE_BOUND));
    }
    let ms = line.strip_prefix("settle ")?;
    Some(ms.trim().parse().map(Duration::from_millis).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "settle expects a timeout in milliseconds (for example: settle 10000)",
        )
    }))
}

/// Script text → key events. `<token>` forms are the special keys
/// every other character feeds as `Char`. An unknown `<...>` feeds
/// literally — scripts never silently lose input.
pub fn parse_keys(text: &str) -> Vec<KeyEvent> {
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        if c != '<' {
            out.push(key(KeyCode::Char(c)));
            continue;
        }
        let token = rest.find('>').map(|end| (&rest[..end], end));
        let code = match token.map(|(tok, _)| tok) {
            Some("esc") => Some(KeyCode::Esc),
            Some("cr") => Some(KeyCode::Enter),
            Some("bs") => Some(KeyCode::Backspace),
            Some("tab") => Some(KeyCode::Tab),
            Some("space") => Some(KeyCode::Char(' ')),
            Some("up") => Some(KeyCode::Up),
            Some("down") => Some(KeyCode::Down),
            Some("left") => Some(KeyCode::Left),
            Some("right") => Some(KeyCode::Right),
            _ => None,
        };
        match (code, token) {
            (Some(code), Some((_, end))) => {
                out.push(key(code));
                rest = &rest[end + 1..];
            }
            // Unknown or unterminated `<` — feed it as a literal char.
            _ => out.push(key(KeyCode::Char('<'))),
        }
    }
    out
}

/// CLI entry (main.rs): build the app exactly like the TUI would —
/// same config/theme/repo handling — then run the script and exit.
/// Runs before any terminal setup; there is nothing to restore.
pub fn run_cli(cli: &crate::cli::Cli) -> std::io::Result<()> {
    let path = cli.headless.as_ref().expect("run_cli requires --headless");
    let script = if path.as_os_str() == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin().lock(), &mut s)?;
        s
    } else {
        std::fs::read_to_string(path)?
    };
    let (mut config, config_warning) = match &cli.config {
        Some(path) => crate::config::Config::load_from(path),
        None => crate::config::Config::load(),
    };
    // Deterministic: the 24h-cached update probe is a network call
    // whose result would leak into `state` — scripts never see it.
    config.update.check = false;
    let theme = cli.resolve_theme(&config);
    let (tx, rx) = crate::event::channel();
    let mut app = App::new(tx, config, theme);
    app.config_warning(config_warning);
    // `rootle owner/repo[@ref]`: same direct-open as the TUI.
    if let Some((owner, name, ref_)) = cli.repo_parts() {
        if let Some(r) = ref_ {
            app.handle_action(crate::action::Action::RefsCommit(r));
        }
        app.handle_action(crate::action::Action::RepoSelected { owner, name });
    }
    let dim = |key: &str, default: u16| {
        std::env::var(key)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    app.record_trace_state("startup");
    let mut driver = Headless::new(
        app,
        rx,
        dim("ROOTLE_HEADLESS_COLS", DEFAULT_COLS),
        dim("ROOTLE_HEADLESS_ROWS", DEFAULT_ROWS),
    );
    // Let the launch flow warm (recents/repos fetch) before step one —
    // the same real tracking and bound as any `settle` step: a launch
    // that wedges fails the run instead of scripting into a loading
    // frame.
    driver.settle(SETTLE_BOUND)?;
    let mut out = std::io::stdout().lock();
    let result = driver.run_script(&script, &mut out);
    out.flush()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offline_driver(script_cols_rows: (u16, u16)) -> Headless {
        let (tx, rx) = crate::event::channel();
        let app = App::with(crate::state::State::default(), tx);
        Headless::new(app, rx, script_cols_rows.0, script_cols_rows.1)
    }

    #[test]
    fn keys_token_forms() {
        let keys = parse_keys("jj<esc><cr>a<bs><tab><space><left><right><up><down>");
        let codes: Vec<KeyCode> = keys.into_iter().map(|k| k.code).collect();
        assert_eq!(
            codes,
            vec![
                KeyCode::Char('j'),
                KeyCode::Char('j'),
                KeyCode::Esc,
                KeyCode::Enter,
                KeyCode::Char('a'),
                KeyCode::Backspace,
                KeyCode::Tab,
                KeyCode::Char(' '),
                KeyCode::Left,
                KeyCode::Right,
                KeyCode::Up,
                KeyCode::Down,
            ]
        );
    }

    #[test]
    fn unknown_tokens_feed_literally() {
        let keys = parse_keys("<nope><dangling");
        let text: String = keys
            .into_iter()
            .map(|k| match k.code {
                KeyCode::Char(c) => c,
                other => panic!("expected chars, got {other:?}"),
            })
            .collect();
        assert_eq!(text, "<nope><dangling");
    }

    #[test]
    fn frame_renders_launch_popup() {
        let mut driver = offline_driver((80, 24));
        let mut out = Vec::new();
        driver.run_script("frame\n", &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("─── frame 80×24"), "banner: {out}");
        // Fresh offline state opens the repo search popup.
        assert!(out.contains("search offline"), "launch popup: {out}");
    }

    #[test]
    fn state_reports_mode_and_quit() {
        let mut driver = offline_driver((80, 24));
        let mut out = Vec::new();
        // The launch popup opens in INSERT; Esc Esc closes it
        // (INSERT→NORMAL→close — headless feeds discrete key events,
        // the PTY's byte-merging caveat doesn't exist here).
        driver
            .run_script("state\nkeys <esc><esc>\nstate\n", &mut out)
            .unwrap();
        let out = String::from_utf8(out).unwrap();
        let states: Vec<&str> = out.lines().filter(|l| l.starts_with("─── state")).collect();
        assert_eq!(states.len(), 2, "{out}");
        assert!(states[0].contains("\"popup\":true"), "{out}");
        assert!(states[0].contains("\"mode\":\"INSERT\""), "{out}");
        assert!(states[1].contains("\"popup\":false"), "{out}");
        assert!(states[1].contains("\"mode\":\"BROWSE\""), "{out}");
    }

    #[test]
    fn quit_stops_the_driver() {
        let mut driver = offline_driver((80, 24));
        let mut out = Vec::new();
        // Esc Esc closes the launch popup, q quits; the trailing
        // frame/state must never render.
        driver
            .run_script("keys <esc><esc>\nkeys q\nframe\nstate\n", &mut out)
            .unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("─── frame"), "frame after quit: {out}");
        assert!(!out.contains("─── state"), "state after quit: {out}");
    }

    #[test]
    fn leader_yank_surfaces_in_state() {
        let (tx, rx) = crate::event::channel();
        let mut app = App::with(
            crate::state::State {
                recent_orgs: vec!["ratatui".into()],
                ..Default::default()
            },
            tx,
        );
        app.handle_action(crate::action::Action::OrgSelected("ratatui".into()));
        let mut driver = Headless::new(app, rx, 80, 24);
        let mut out = Vec::new();
        // ␣ y with the offline provider: no URL exists, and the
        // status line must say so honestly (recording coverage with
        // real URLs lives in e2e/test_headless.py over fs_provider).
        driver
            .run_script("keys <space>y\nstate\n", &mut out)
            .unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("nothing to yank"), "status in state: {out}");
    }

    #[test]
    fn settle_deadline_fails_with_outstanding_count() {
        // A worker that never finishes: settle fails at its bound,
        // names the stuck count, and the script stops there — the
        // trailing frame must never render.
        let mut driver = offline_driver((80, 24));
        let ticket = driver.app.outstanding_workers().track();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let parked = std::thread::spawn(move || {
            let _held = ticket; // in flight until the test ends
            let _ = released.recv();
        });
        let mut out = Vec::new();
        let failure = driver
            .run_script("settle 25\nframe\n", &mut out)
            .unwrap_err();
        assert_eq!(failure.kind(), std::io::ErrorKind::TimedOut);
        let out = String::from_utf8(out).unwrap();
        assert!(
            !out.contains("─── frame"),
            "frame after settle failure: {out}"
        );
        drop(release);
        let _ = parked.join();
    }

    #[test]
    fn malformed_settle_timeout_stops_before_sampling() {
        let mut driver = offline_driver((80, 24));
        let mut out = Vec::new();
        let failure = driver
            .run_script("settle nope\nstate\n", &mut out)
            .unwrap_err();
        assert_eq!(failure.kind(), std::io::ErrorKind::InvalidInput);
        assert!(out.is_empty());
    }
}
