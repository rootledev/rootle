"""Generic PTY harness for driving the rootle TUI end to end.

Spawns the real binary on a pseudo-terminal, injects keystrokes,
reconstructs the visible screen with pyte, and asserts on text —
the live complement to the TestBackend snapshot tests (see
.agents/skills/rootle-tui-debug).

Rules baked in from the skill:
- The PTY window size is set before spawn (a 0x0 PTY draws nothing).
- ESC is sent one keystroke per call; back-to-back ESC bytes can merge
  into Alt+<key> in crossterm's parser and look like a bug.
- Each session is hermetic: HOME and the XDG dirs point at a temp dir,
  so tests never touch the developer's real state/cache/config.
"""

from __future__ import annotations

import fcntl
import os
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time
from pathlib import Path

import pyte

ROOT = Path(__file__).resolve().parent.parent
BINARY = ROOT / "target" / "debug" / "rootle"

KEYS: dict[str, bytes] = {
    "BACKSPACE": b"\x7f",
    "ENTER": b"\r",
    "ESC": b"\x1b",
    "TAB": b"\t",
    "BACKTAB": b"\x1b[Z",
    "UP": b"\x1b[A",
    "DOWN": b"\x1b[B",
    "LEFT": b"\x1b[D",
    "RIGHT": b"\x1b[C",
}


def build() -> Path:
    """Build the debug binary once per test session.

    Inside the docker `e2e` service the gate stage already compiled it
    (ROOTLE_E2E_IN_DOCKER) — reusing that artifact is the point of the
    compose bootstrap; on the host we build with cargo as before.
    """
    if os.environ.get("ROOTLE_E2E_IN_DOCKER"):
        assert BINARY.exists(), f"gate stage should have built {BINARY}"
        return BINARY
    subprocess.run(["cargo", "build", "--quiet"], cwd=ROOT, check=True)
    return BINARY


class Tui:
    """One rootle instance on a PTY, screen reconstructed via pyte."""

    def __init__(
        self,
        binary: Path,
        cols: int = 120,
        rows: int = 36,
        args: list[str] | None = None,
        env_extra: dict[str, str] | None = None,
    ) -> None:
        self.binary = binary
        self.cols = cols
        self.rows = rows
        self.args = args or []
        self.env_extra = env_extra or {}
        self._screen = pyte.Screen(cols, rows)
        self._stream = pyte.ByteStream(self._screen)
        self._master: int | None = None
        self._proc: subprocess.Popen[bytes] | None = None
        self._home = tempfile.TemporaryDirectory(prefix="rootle-e2e-")
        # asciinema v2 recording (demo capture): header + [dt, "o", text].
        self._recording: list | None = None
        self._rec_clock: float | None = None

    # -- lifecycle ------------------------------------------------------

    def start(self) -> Tui:
        master, slave = pty.openpty()
        fcntl.ioctl(
            slave, termios.TIOCSWINSZ, struct.pack("HHHH", self.rows, self.cols, 0, 0)
        )
        env = dict(os.environ)
        home = Path(self._home.name)
        env.update(
            HOME=str(home),
            XDG_CONFIG_HOME=str(home / "config"),
            XDG_CACHE_HOME=str(home / "cache"),
            XDG_STATE_HOME=str(home / "state"),
            # Editor-open tests land on `true` — a no-op that returns
            # instantly, exercising only the suspend/resume path.
            VISUAL="true",
            EDITOR="true",
        )
        env.pop("ROOTLE_CONFIG", None)
        env.update(self.env_extra)
        self._proc = subprocess.Popen(
            [str(self.binary), *self.args],
            stdin=slave,
            stdout=slave,
            stderr=slave,
            env=env,
            close_fds=True,
        )
        os.close(slave)
        self._master = master
        return self

    # -- recording (asciinema v2 cast) -----------------------------------

    def record(self) -> None:
        """Start capturing output events for an asciinema cast."""
        import time as _t

        self._recording = [
            {
                "version": 2,
                "width": self.cols,
                "height": self.rows,
                "timestamp": int(_t.time()),
                "env": {"TERM": "xterm-256color", "SHELL": "/bin/sh"},
            }
        ]
        self._rec_clock = _t.monotonic()

    def _feed(self, chunk: bytes) -> None:
        """Feed the VT emulator (and the recorder, if armed)."""
        import time as _t

        self._stream.feed(chunk)
        if self._recording is not None and self._rec_clock is not None:
            now = _t.monotonic()
            self._recording.append([round(now - self._rec_clock, 3), "o", chunk.decode("utf-8", "replace")])
            self._rec_clock = now

    def save_recording(self, path) -> None:
        """Write the cast file; no-op when not recording."""
        import json as _j

        if not self._recording:
            return
        with open(path, "w") as f:
            for i, event in enumerate(self._recording):
                f.write(_j.dumps(event) + ("\n" if True else ""))
        self._recording = None

    def stop(self) -> None:
        if self._proc and self._proc.poll() is None:
            self._proc.terminate()
            try:
                self._proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self._proc.kill()
        if self._master is not None:
            os.close(self._master)
            self._master = None
        self._home.cleanup()

    def __enter__(self) -> Tui:
        return self.start()

    def __exit__(self, *_: object) -> None:
        self.stop()

    # -- input ----------------------------------------------------------

    def send(self, text: str, settle: float = 0.08) -> None:
        self._write(text.encode())
        self._settle(quiet=settle)

    def key(self, name: str, settle: float = 0.08) -> None:
        self._write(KEYS[name])
        self._settle(quiet=settle)

    def type_query(self, text: str, settle: float = 0.03) -> None:
        """Printable input, one keystroke per call like a real user."""
        for ch in text:
            self.send(ch, settle=settle)

    def _settle(self, quiet: float, max_wait: float = 2.0) -> None:
        """Pump until the app stops repainting (quiet window) instead
        of a fixed sleep — adapts to slow CI containers and stays fast
        when a key is a no-op."""
        assert self._master is not None, "Tui not started"
        deadline = time.monotonic() + max_wait
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self._master], [], [], quiet)
            if not ready:
                return
            try:
                chunk = os.read(self._master, 65536)
            except OSError:
                return
            if not chunk:
                return
            self._feed(chunk)

    def _write(self, data: bytes) -> None:
        assert self._master is not None, "Tui not started"
        os.write(self._master, data)

    # -- output ---------------------------------------------------------

    def _pump(self, timeout: float = 0.05) -> None:
        assert self._master is not None, "Tui not started"
        deadline = time.monotonic() + timeout
        while True:
            wait = deadline - time.monotonic()
            if wait <= 0:
                return
            ready, _, _ = select.select([self._master], [], [], wait)
            if not ready:
                return
            try:
                chunk = os.read(self._master, 65536)
            except OSError:
                return  # child exited, PTY closed
            if not chunk:
                return
            self._feed(chunk)

    def screen(self) -> str:
        self._pump()
        return "\n".join(self._screen.display)

    def expect(self, needle: str, timeout: float = 5.0) -> str:
        """Poll until `needle` is visible; on timeout dump the screen."""
        deadline = time.monotonic() + timeout
        while True:
            screen = self.screen()
            if needle in screen:
                return screen
            if time.monotonic() > deadline:
                raise AssertionError(
                    f"expected {needle!r} within {timeout}s; screen was:\n{screen}"
                )
            time.sleep(0.05)

    def expect_gone(self, needle: str, timeout: float = 5.0) -> str:
        deadline = time.monotonic() + timeout
        while True:
            screen = self.screen()
            if needle not in screen:
                return screen
            if time.monotonic() > deadline:
                raise AssertionError(
                    f"expected {needle!r} gone within {timeout}s; screen was:\n{screen}"
                )
            time.sleep(0.05)
