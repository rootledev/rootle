"""The headless driver (plans/0023 M1) against the real binary.

Same flows as the PTY suite, without the terminal: keys in, cell-grid
frames + state JSON out. This is the tier behavioral tests default to
— deterministic, no pyte, no settle heuristics; the PTY suite keeps
what only a terminal proves (byte parsing, terminal restore).
"""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

from conftest import FS_PROVIDER, make_fs_root
from tui import build, hermetic_env


def run_headless(
    binary: Path, script: str, *args: str, home: Path, cols: int = 100, rows: int = 30
) -> str:
    """Run `rootle --headless -` with the script on stdin; return stdout."""
    home.mkdir(parents=True, exist_ok=True)
    env = hermetic_env(home)
    env["ROOTLE_HEADLESS_COLS"] = str(cols)
    env["ROOTLE_HEADLESS_ROWS"] = str(rows)
    proc = subprocess.run(
        [str(binary), "--headless", "-", *args],
        input=script.encode(),
        capture_output=True,
        env=env,
        timeout=60,
    )
    assert proc.returncode == 0, proc.stderr.decode()
    return proc.stdout.decode()


def fs_config(tmp: Path) -> Path:
    root = make_fs_root(tmp)
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


def test_browse_search_drill_yank(tmp_path, binary):
    """The PTY suite's canonical flow, headless: search alpha, open the
    tree, drill to a file, yank its URL — all via real provider IPC."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys alpha\n"
        "keys <cr>\n"
        "settle\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n"
        "keys <cr>\n"  # drill src/ — selection starts on it (dirs first)
        "settle\n"
        "frame\n"
        "keys <space>y\n"
        "state\n"
        "keys q\n",
        "--config",
        str(config),
        home=tmp_path / "home",
    )
    assert "README.md" in out and "local · alpha" in out
    assert "fn main" not in out.split("─── frame")[1]  # tree frame: no preview yet
    assert "println!" in out  # drilled frame shows the blob
    (state,) = states(out)
    assert state["provider"] == "stdio:fs"
    assert state["yanks"], out
    assert "alpha/src/main.rs" in state["yanks"][0]
    assert state["should_quit"] is False  # q came after the state step


def test_state_reports_launch_flow(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "state\nkeys <esc><esc>\nstate\n",
        "--config",
        str(config),
        home=tmp_path / "home",
    )
    first, second = states(out)
    assert first["popup"] is True and first["mode"] == "INSERT"
    assert second["popup"] is False and second["mode"] == "BROWSE"


def test_output_is_plain_text(tmp_path, binary):
    """Headless writes to pipes: no ANSI escapes, no terminal setup —
    output must be diff-able plain text for reviewers and agents."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary, "frame\n", "--config", str(config), home=tmp_path / "home"
    )
    assert "\x1b" not in out
    assert "─── frame 100×30" in out


def test_viewport_override(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=60,
        rows=15,
    )
    assert "─── frame 60×15" in out


EMBEDDED_THEMES = (
    "catppuccin-mocha",
    "dracula",
    "gruvbox-dark",
    "nord",
    "one-dark",
    "solarized-dark",
    "tokyo-night",
    "catppuccin-latte",
    "github-light",
    "one-light",
    "solarized-light",
)


def test_embedded_themes_in_settings_radio(tmp_path, binary):
    """Ported from the retired PTY test_themes.py (plans/0023): pure
    screen-content-after-keys — exactly what the headless tier is for.
    """
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys <esc><esc>\n"  # close the launch popup (INSERT→NORMAL→close)
        "keys :settings<cr>\n"
        "keys <tab><cr>\n"  # editor → theme section, open the radio
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
    )
    for name in EMBEDDED_THEMES:
        assert name in out, f"{name} missing from the radio"


# --- Declared-provider lifecycle (ported from the PTY test_declared.py) ------
#
# The decline path re-warms the fallback provider; with network
# enforced off (hermetic_env's discard-port proxy), that fetch error
# deterministically covers the transient modeline slot — the sticky
# degraded slot is the honest surface, and `state` observes it
# directly. This is the macOS-CI race, closed by construction.


def test_declared_missing_asks_then_degrades_honestly(tmp_path, binary):
    config = tmp_path / "declared.toml"
    config.write_text('[provider]\nkind = "gitlab"\n')
    out = run_headless(
        binary,
        "frame\nkeys n\nsettle\nstate\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=170,
    )
    assert "install provider?" in out
    assert "config declares gitlab" in out
    assert "you are trusting rootledev/rootle-gitlab" in out
    assert "y install" in out
    (state,) = states(out)
    assert state["consent"] is False
    degraded = state["degraded"] or ""
    assert "gitlab not installed" in degraded
    assert "browsing github" in degraded
    # The declaration stays in the config, untouched.
    assert 'kind = "gitlab"' in config.read_text()


def test_declared_pin_fields_surface_in_the_popup(tmp_path, binary):
    config = tmp_path / "pinned.toml"
    config.write_text(
        '[provider]\nkind = "bitbucket"\ntag = "v0.1.4"\n'
        'sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"\n'
    )
    out = run_headless(
        binary,
        "frame\nkeys n\nsettle\nstate\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=170,
    )
    assert "config declares bitbucket" in out
    assert "rootledev/rootle-bitbucket" in out
    assert "tag v0.1.4" in out
    assert "sha256" in out
    (state,) = states(out)
    assert "browsing github" in (state["degraded"] or "")


def test_broken_stdio_raises_health_prompt(tmp_path, binary):
    """0022 M2: a spawn that fails at startup raises the health prompt
    (r/g/e); `g` degrades to github with the notice sticky."""
    config = tmp_path / "broken.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\ncommand = ["/nonexistent/no-such-provider"]\n'
    )
    out = run_headless(
        binary,
        "frame\nkeys g\nsettle\nstate\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=170,
    )
    assert "provider health" in out
    assert "failed to start" in out
    assert "r retry" in out
    (state,) = states(out)
    degraded = state["degraded"] or ""
    assert "not installed" in degraded or "failed to start" in degraded
    assert "provider install" in degraded or "browsing github" in degraded


def test_tarball_kind_health_no_retry(tmp_path, binary):
    """A kind naming a plain-HTTP tarball is never auto-fetched (0019
    rule) and never retryable: the health prompt offers g/e only."""
    config = tmp_path / "tarball.toml"
    config.write_text(
        '[provider]\nkind = "https://artifacts.corp.example/rootle-x.tar.gz"\n'
    )
    out = run_headless(
        binary,
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=170,
    )
    assert "provider health" in out
    assert "g browse github" in out
    assert "r retry" not in out
