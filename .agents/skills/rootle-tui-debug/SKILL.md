---
name: rootle-tui-debug
description: End-to-end debugging and verification workflow for the rootle ratatui TUI — run it in a PTY, inject keystrokes, capture frames, and verify rendering integrity (no lingering cells, correct modes, sanitization). Use whenever verifying or debugging rootle's terminal behavior.
---

# rootle TUI end-to-end debugging

Three complementary verification paths. Use the first two for every
behavioral change; hub PTY is for ad-hoc poking only:

1. **e2e harness** (`e2e/`, uv + pytest + pyte) — scripted PTY runs
   against the real binary with screen reconstruction and assertions.
2. **Frame snapshots** (`tests/render.rs`, ratatui `TestBackend`) —
   deterministic frame content without a TTY.
3. **hub PTY** — manual/ad-hoc interaction when the above don't fit.

## 1. e2e harness (`e2e/`)

The project has a permanent, uv-managed PTY harness. Prefer it over
hand-rolled PTY scripts — it's hermetic and repeatable:

- Run the suite: `cd e2e && uv run pytest` (builds the debug binary
  itself via a session fixture), or dockerized — reusing the `test`
  gate's compile cache with zero host artifacts:
  `docker compose run --build --rm e2e` (`--build` is required after
  source changes, same as every compose service).
- `e2e/tui.py` — generic driver: `Tui(binary, cols, rows)`, `send()` /
  `key()` / `type_query()`, `expect()` / `expect_gone()` with screen
  dumps on timeout, `screen()` returns the pyte-reconstructed frame.
  Sessions are hermetic: HOME and XDG dirs point at a temp dir, so
  tests never touch real state/cache/config; `VISUAL=true` makes the
  editor-open path an instant no-op (suspend/resume still exercised).
- `e2e/conftest.py` — `tui` fixture (one instance per test) and
  `dismiss_launch_popup()` for the launch flow.
- New generic TUI behaviors (a flow any user could hit) belong in
  `e2e/`; component-edge cases belong in Rust unit tests.

Hub PTY remains useful for quick manual checks:

```
hub(op="start", name="rootle", application="cargo", args=["run", "--quiet"],
```

- Inject keys: `hub(op="send", name="rootle", text="jj")` for printable
  input; `keys=["ENTER"]` / `["ESCAPE"]` / `["TAB"]` for special keys.
  Send ESC **one call at a time** — back-to-back `\x1b` bytes can merge
  into `Alt+<key>` in crossterm's parser and look like a bug.
- Read output: `hub(op="logs", name="rootle")`. Output contains raw ANSI —
  reconstruct the visible screen via the harness/pyte instead of
  eyeballing escapes (see §3).
- Stop: `hub(op="stop", name="rootle")` (or send `q`).

The binary must never leave the terminal in raw mode or the alternate
screen on exit — after `q`, the shell prompt must reappear normally.
If the app panics, check the panic hook restored the terminal before
the message printed.
## 2. Frame snapshots (deterministic, no TTY)

Use ratatui's `TestBackend` to render the app into a `Buffer`, then
assert or print rows:

```rust
let backend = TestBackend::new(100, 30);
let mut terminal = Terminal::new(backend).unwrap();
terminal.draw(|f| app.render(f, f.area())).unwrap();
let buf = terminal.backend().buffer();
for y in 0..buf.area.height {
    let row: String = (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol()).collect();
    println!("{row}");
}
```

Rules:

- Every new screen/popup/mode gets a snapshot test in `tests/` or a
  `#[cfg(test)]` module next to the component.
- After a popup closes, re-render and assert the cells under the popup
  match the pre-popup frame (catches lingering-text regressions).
- Resize (`TestBackend` at a new size) and re-render: assert no panic
  and modeline still occupies the last row.

## 3. Reconstructing a live screen

`hub logs` render ANSI-stripped text, which mangles frame diffs. For a
ground-truth view, use the uv-managed harness in `e2e/` (pyte-backed):

- `cd e2e && uv run pytest` runs the PTY e2e suite; `e2e/tui.py` is the
  generic driver (hermetic HOME/XDG, winsize set pre-spawn, `expect()` /
  `expect_gone()` assertions). Add generic TUI scenarios there.
- For one-off captures: `uv run python` from `e2e/`, drive `Tui`, print
  `tui.screen()` — pyte reconstructs the frame.
- No uv/pyte available? A ~60-line Python VT emulator handling CUP,
  CUU/CUD/CUF/CUB, ED(2J), EL(K), CR/LF and printable chars is enough
  for ratatui output. The harness (`e2e/tui.py`) already sets the
  window size with `fcntl.ioctl(slave, termios.TIOCSWINSZ, …)` before
  spawn — a 0×0 PTY makes ratatui draw nothing at all (looks like a
  hang). Write keys with sleeps, read output with `select`.
- Worker/backend events: set `ROOTLE_TRACE=/tmp/rootle_trace.log` in the
  child env — spawn/results/selection lines with timestamps.

## 4. Rendering-integrity checklist

For any change touching render code, verify:

1. Popup open → close: no residue (snapshot test §2).
2. Editor/child-process resume path: full `terminal.clear()` happened.
3. File content with ESC/control bytes renders stripped (see
   `src/sanitize.rs`); binary blob shows the placeholder, not bytes.
4. Wide chars (CJK) truncate by display width, not byte/char count.
5. Resize while a popup is open: popup recenters, modeline intact.

## 5. Input debugging

VimInput state machine bugs: drive key sequences through
`VimInput::handle_key` in a unit test rather than the PTY — the PTY
makes keystroke timing flaky. PTY is only for the final integrated
behavior (focus switching with Tab, Esc dismissal).
