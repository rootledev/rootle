---
name: ghx-tui-debug
description: End-to-end debugging and verification workflow for the ghx ratatui TUI — run it in a PTY, inject keystrokes, capture frames, and verify rendering integrity (no lingering cells, correct modes, sanitization). Use whenever verifying or debugging ghx's terminal behavior.
---

# ghx TUI end-to-end debugging

Two complementary verification paths. Use BOTH for behavioral changes:
snapshot tests for deterministic frame content, live PTY for real
terminal lifecycle.

## 1. Live PTY run (real terminal)

Start the app in a project-scoped PTY process:

```
hub(op="start", name="ghx", application="cargo", args=["run", "--quiet"],
Then:

- Inject keys: `hub(op="send", name="ghx", text="jj")` for printable
  input; `keys=["ENTER"]` / `["ESCAPE"]` / `["TAB"]` for special keys.
  Send ESC **one call at a time** — back-to-back `\x1b` bytes can merge
  into `Alt+<key>` in crossterm's parser and look like a bug.
- Read output: `hub(op="logs", name="ghx")`. Output contains raw ANSI —
  to reconstruct the visible screen, pipe through a terminal emulator
  parser instead of eyeballing escapes (see §3).
- Stop: `hub(op="stop", name="ghx")` (or send `q`).

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
ground-truth view, drive ghx on a raw PTY yourself and replay the
capture through a VT emulator:

- Preferred: `pyte` (`pip install pyte`). Feed the raw capture to
  `pyte.Screen(120, 36)` and print `screen.display`.
- No pyte/pip available? A ~60-line Python VT emulator handling CUP,
  CUU/CUD/CUF/CUB, ED(2J), EL(K), CR/LF and printable chars is enough
  for ratatui output (ghx's `/tmp/vt.py` pattern).
- Deterministic driver (preferred over `script`, which breaks on piped
  stdin): Python `pty.fork()`, **set the window size with
  `fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 36, 120, 0, 0))`**
  — a 0×0 PTY makes ratatui draw nothing at all (looks like a hang).
  Write keys with sleeps, read output with `select`, reconstruct with
  the VT emulator.
- Worker/backend events: set `GHX_TRACE=/tmp/ghx_trace.log` in the
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
