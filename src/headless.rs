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
//! settle        drain provider/worker events to quiescence
//!               (400ms quiet window, 10s bound)
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

/// Default script viewport — `ROOTLE_HEADLESS_COLS` /
/// `ROOTLE_HEADLESS_ROWS` override (pane-layout stress needs sizes).
pub const DEFAULT_COLS: u16 = 100;
pub const DEFAULT_ROWS: u16 = 30;

/// `settle`'s quiet window: provider workers deliver in chains
/// (tree → blob → last-commit); the channel must stay quiet this long
/// before the step returns. Sized for a cold local-provider round
/// trip under CI load — work *in flight* is invisible to the
/// channel, so the window is the only signal (strop's "zero timing"
/// ideal bends the same way for real data sources).
const SETTLE_QUIET: Duration = Duration::from_millis(400);
/// …but never longer than this — a stalled provider must not wedge
/// the driver (0020 §4's honesty rule applies to scripts too).
const SETTLE_BOUND: Duration = Duration::from_secs(10);

pub struct Headless {
    app: App,
    rx: AppRx,
    cols: u16,
    rows: u16,
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
        self.collect_side_effects();
        drained
    }

    /// The main loop's out-of-draw side effects, recorded instead of
    /// executed: no clipboard to write, no editor to suspend into.
    /// Exception: `ROOTLE_CLIPBOARD=<path>` (the e2e/CI override) is a
    /// plain file write, not a terminal escape — honored for fidelity.
    fn collect_side_effects(&mut self) {
        if let Some(job) = self.app.take_editor_job() {
            let mut cmd = job.program;
            if !job.args.is_empty() {
                cmd.push(' ');
                cmd.push_str(&job.args.join(" "));
            }
            self.editor_jobs.push(cmd);
        }
        if let Some(text) = self.app.take_clipboard() {
            if std::env::var_os("ROOTLE_CLIPBOARD").is_some() {
                crate::clipboard::copy(&text);
            }
            self.yanks.push(text);
        }
    }

    /// Drain until the channel stays quiet for `SETTLE_QUIET`,
    /// bounded by `SETTLE_BOUND`.
    fn settle(&mut self) {
        let deadline = Instant::now() + SETTLE_BOUND;
        let mut quiet_since = Instant::now();
        loop {
            if self.drain() {
                quiet_since = Instant::now();
            }
            if quiet_since.elapsed() >= SETTLE_QUIET || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
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
        let backend = TestBackend::new(self.cols, self.rows);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| {
                let area = f.area();
                self.app.render(f, area);
            })
            .expect("draw");
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..self.rows {
            let row: String = (0..self.cols).map(|x| buf[(x, y)].symbol()).collect();
            out.push_str(row.trim_end());
            out.push('\n');
        }
        out
    }

    pub fn state_json(&self) -> String {
        let mut state = self.app.snapshot();
        state["editor_jobs"] = self.editor_jobs.clone().into();
        state["yanks"] = self.yanks.clone().into();
        state.to_string()
    }

    /// Interpret a script (module docs describe the language),
    /// writing frames/states to `out`.
    pub fn run_script(&mut self, script: &str, out: &mut dyn Write) {
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
            } else if line == "settle" {
                self.settle();
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
    }
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
    let mut config = match &cli.config {
        Some(path) => crate::config::Config::load_from(path),
        None => crate::config::Config::load(),
    };
    // Deterministic: the 24h-cached update probe is a network call
    // whose result would leak into `state` — scripts never see it.
    config.update.check = false;
    let theme = cli.resolve_theme(&config);
    let (tx, rx) = crate::event::channel();
    let mut app = App::new(tx, config, theme);
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
    let mut driver = Headless::new(
        app,
        rx,
        dim("ROOTLE_HEADLESS_COLS", DEFAULT_COLS),
        dim("ROOTLE_HEADLESS_ROWS", DEFAULT_ROWS),
    );
    // Let the launch flow warm (recents/repos fetch) before step one.
    driver.settle();
    let mut out = std::io::stdout().lock();
    driver.run_script(&script, &mut out);
    out.flush()
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
        driver.run_script("frame\n", &mut out);
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
        driver.run_script("state\nkeys <esc><esc>\nstate\n", &mut out);
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
        driver.run_script("keys <esc><esc>\nkeys q\nframe\nstate\n", &mut out);
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
        driver.run_script("keys <space>y\nstate\n", &mut out);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("nothing to yank"), "status in state: {out}");
    }
}
