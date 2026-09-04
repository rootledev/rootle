"""Shared helpers for the headless tier (plans/0023): run the real
binary via `rootle --headless -` (script on stdin) and parse the
frames + state JSON it prints. No PTY, no pyte, deterministic.

Script language (src/headless.rs module docs): `keys <text>` (token
forms `<esc> <cr> <bs> <tab> <space> <up|down|left|right>`), `settle`
(drain workers to quiescence), `wait <ms>`, `frame`, `state`,
`#` comments.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

from conftest import FS_PROVIDER, make_fs_root
from tui import hermetic_env


def run_headless(
    binary: Path,
    script: str,
    *args: str,
    home: Path,
    cols: int = 100,
    rows: int = 30,
    env_extra: dict[str, str] | None = None,
) -> str:
    """Run `rootle --headless -` with the script on stdin; return stdout.

    The process runs in its own group and a timeout kills the GROUP:
    rootle's provider children hold the stdout pipe, and a lone-child
    kill would leave `communicate()` waiting on an orphan forever
    (the macOS CI hang)."""
    import os
    import signal

    home.mkdir(parents=True, exist_ok=True)
    env = hermetic_env(home, env_extra)
    env["ROOTLE_HEADLESS_COLS"] = str(cols)
    env["ROOTLE_HEADLESS_ROWS"] = str(rows)
    proc = subprocess.Popen(
        [str(binary), "--headless", "-", *args],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        start_new_session=True,
    )
    try:
        stdout, stderr = proc.communicate(input=script.encode(), timeout=120)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        stdout, stderr = proc.communicate()
        raise AssertionError(
            f"headless run wedged (120s); partial stderr:\n{stderr.decode()}"
        )
    assert proc.returncode == 0, stderr.decode()
    return stdout.decode()


def fs_config(tmp: Path, root: Path | None = None) -> Path:
    """A stdio config over the fs reference provider. Pass an explicit
    root (e.g. conftest.make_git_root) or get the default fs fixture."""
    root = root or make_fs_root(tmp)
    config = tmp / "config.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    return config


def states(output: str) -> list[dict]:
    """The parsed JSON of every `─── state {...}` line."""
    out = []
    for line in output.splitlines():
        if line.startswith("─── state "):
            out.append(json.loads(line.removeprefix("─── state ")))
    return out


def frames(output: str) -> list[str]:
    """Each `frame` step's cell grid (banner line excluded)."""
    parts = output.split("─── frame ")
    return [p.split("\n", 1)[1] for p in parts[1:]]
