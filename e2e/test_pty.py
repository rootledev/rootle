"""PTY-only smoke (plans/0023 tier 3): what only a terminal proves.

Behavioral coverage lives in the headless tier (e2e/headless.py) and
the frame tests; this file pins the terminal boundary itself:
alternate-screen enter/leave, exit code, merged-ESC byte parsing,
editor suspend/resume, and resize redraw.
"""

from __future__ import annotations

import fcntl
import os
import struct
import termios
from pathlib import Path

import pyte
from conftest import dismiss_launch_popup, open_fs_repo
from tui import Tui, build

ALT_ENTER = b"\x1b[?1049h"
ALT_LEAVE = b"\x1b[?1049l"


def provider_tui(tmp_path: Path, **kwargs) -> Tui:
    """fs-provider instance, like conftest.provider_tui but manual."""
    from conftest import FS_PROVIDER, make_fs_root

    root = make_fs_root(tmp_path)
    config = tmp_path / "p.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    return Tui(build(), args=["--config", str(config)], **kwargs).start()


def test_launch_and_quit_restores_terminal(tmp_path: Path) -> None:
    tui = provider_tui(tmp_path)
    try:
        tui.expect("search ")
        raw = tui.raw()
        assert ALT_ENTER in raw, "app must enter the alternate screen"
        tui.key("ESC")
        tui.key("ESC")
        tui.send("q")
        code = tui.wait_exit()
        assert code == 0, f"exit code {code}"
        raw = tui.raw()
        assert ALT_LEAVE in raw, "app must leave the alternate screen"
        # Cursor style reset: the app never leaves a bar/block behind.
        assert b"\x1b[0 q" in raw or b"\x1b[ q" in raw, "cursor shape reset"
    finally:
        tui.stop()


def test_merged_esc_bytes_do_not_wedge(tmp_path: Path) -> None:
    """Two ESC bytes in ONE write: crossterm's parser holds the
    trailing \\x1b awaiting a disambiguating byte (Esc vs Alt+…).
    rootle's contract is that the parser never WEDGES — the next key
    flushes the pending Esc and the popup still closes. Byte-level
    parsing is the one thing the headless tier skips by design."""
    tui = provider_tui(tmp_path)
    try:
        tui.expect("search ")
        os.write(tui._master, b"\x1b\x1b")  # merged: INSERT → NORMAL, one held
        tui.key("ESC")  # flushes the held byte + this one → close
        tui.expect_gone("search ")
        # And the app still processes keys afterwards (no stuck state).
        tui.send("j")
        assert "BROWSE" in tui.screen()
    finally:
        tui.stop()



def test_editor_suspend_resume_roundtrip(tmp_path: Path) -> None:
    """Enter on a previewed file suspends the TUI (VISUAL=true no-ops
    the editor itself) and resumes with a full redraw — the one
    legitimate terminal.clear path."""
    tui = provider_tui(tmp_path)
    try:
        open_fs_repo(tui)
        tui.send("l")  # drill src/
        tui.expect("fn main")
        before = tui.raw().count(ALT_LEAVE)
        tui.key("ENTER")  # open main.rs in $EDITOR (true: instant)
        tui.expect("fn main")  # back, content intact
        raw = tui.raw()
        assert raw.count(ALT_LEAVE) > before, "suspend leaves the alt screen"
        assert raw.count(ALT_ENTER) >= 2, "resume re-enters the alt screen"
    finally:
        tui.stop()


def test_resize_redraws_every_cell(tmp_path: Path) -> None:
    tui = provider_tui(tmp_path, cols=100, rows=30)
    try:
        dismiss_launch_popup(tui)
        fcntl.ioctl(
            tui._master,
            termios.TIOCSWINSZ,
            struct.pack("HHHH", 20, 72, 0, 0),  # 72x20
        )
        tui.send("j")  # any key: the next draw is at the new size
        tui._settle(quiet=0.2)
        # Replay the whole byte stream through a fresh emulator sized
        # 72x20 — the modeline must sit on the new last row.
        screen = pyte.Screen(72, 20)
        stream = pyte.ByteStream(screen)
        stream.feed(tui.raw())
        last_row = screen.display[19]
        assert "BROWSE" in last_row, f"modeline not on the new last row: {screen.display!r}"
    finally:
        tui.stop()


def test_dumb_terminal_still_renders(tmp_path: Path) -> None:
    """TERM=dumb: no colors, but the chrome still draws (colors are
    semantic, never a hard dependency)."""
    tui = provider_tui(tmp_path, env_extra={"TERM": "dumb"})
    try:
        tui.expect("search ")
        assert "orgs" in tui.screen()
    finally:
        tui.stop()
