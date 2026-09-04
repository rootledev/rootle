"""Headless-tier e2e for the v0.3/v0.4 wiring (plans/0023: scripted
keys + frame/state dumps, no PTY): yank to clipboard (ROOTLE_CLIPBOARD
file override — honored even headless), settings write-back, and the
clone wizard running real `git clone` through a provider-supplied
clone URL (the fs provider's repos are real git repos here)."""

from __future__ import annotations

import subprocess
from pathlib import Path

from headless import frames, fs_config, run_headless, states


def make_cloneable_root(tmp_path: Path) -> Path:
    """fs root whose repos are real git repos (cloneable via path)."""
    root = tmp_path / "code"
    for name, files in {
        "alpha": {"src/main.rs": "fn render() {}\n"},
        "beta": {"notes.txt": "hi\n"},
    }.items():
        repo = root / name
        (repo / "src").mkdir(parents=True, exist_ok=True) if name == "alpha" else repo.mkdir()
        for rel, text in files.items():
            (repo / rel).write_text(text)
        subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
        subprocess.run(["git", "add", "."], cwd=repo, check=True)
        subprocess.run(
            ["git", "-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"],
            cwd=repo,
            check=True,
        )
    return root


def test_yank_writes_clipboard(tmp_path, binary):
    clip = tmp_path / "clip.txt"
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys alpha\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"
        "keys <space>y\n"
        "state\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
        env_extra={"ROOTLE_CLIPBOARD": str(clip)},
    )
    (state,) = states(out)
    (f,) = frames(out)
    assert "yanked" in f
    assert state["yanks"][0].startswith("file://")  # provider-supplied URL
    assert state["yanks"][0].endswith("/alpha")  # repo root URL yanked
    assert clip.read_text().endswith("/alpha")  # the override file got it


def test_settings_write_back_and_toast(tmp_path, binary):
    config = fs_config(tmp_path)
    home = tmp_path / "home"
    out = run_headless(
        binary,
        "keys <esc><esc>\n"
        "keys :settings<cr>\n"
        "settle\n"
        # editor tab is first; land on cache.max_mb: Tab×2, j×0 (only
        # field), Enter to edit, retype the value, Enter commits.
        "keys <tab><tab><cr>\n"
        "keys <bs><bs><bs>\n"
        "keys 128\n"
        "keys <cr>\n"  # commit field
        "keys <esc>\n"  # close popup → dirty → ApplySettings
        "settle\n"
        "frame\n",
        "--config",
        str(config),
        home=home,
        cols=110,
    )
    (f,) = frames(out)
    assert "settings saved" in f
    # The hermetic XDG config got the written section.
    written = home / "config" / "rootle" / "config.toml"
    assert "max_mb = 128" in written.read_text()


def test_settings_theme_radio_selects_and_saves(tmp_path, binary):
    config = fs_config(tmp_path)
    # A second theme in a hermetic XDG dir; save() also writes there.
    xdg = tmp_path / "xdg"
    themes = xdg / "rootle" / "themes"
    themes.mkdir(parents=True)
    (themes / "gruvbox-dark.toml").write_text('[semantic]\nborder_focused = "#b8bb26"\n')
    out = run_headless(
        binary,
        "keys <esc><esc>\n"
        "keys :settings<cr>\n"
        "settle\n"
        "keys <tab>\n"  # → theme section: radio list of palettes
        "frame\n"
        # Alphabetical: catppuccin-latte → catppuccin-mocha → dracula →
        # github-light → gruvbox-dark.
        + "keys " + "j" * 4 + "\n"
        + "keys <space>\n"  # select gruvbox-dark → live preview + unsaved chip
        + "frame\n"
        "keys <esc>\n"  # dirty → ApplySettings
        "settle\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
        env_extra={"XDG_CONFIG_HOME": str(xdg)},
    )
    f = frames(out)
    assert "gruvbox-dark" in f[0]
    assert "\u25cf catppuccin-mocha" in f[0]  # current theme carries the dot
    assert "unsaved" in f[1]
    assert "\u25cf gruvbox-dark" in f[1]
    assert "\u25cb catppuccin-mocha" in f[1]
    assert "settings saved" in f[2]
    assert 'name = "gruvbox-dark"' in (xdg / "rootle" / "config.toml").read_text()


def test_clone_wizard_runs_git_clone(tmp_path, binary, monkeypatch):
    root = make_cloneable_root(tmp_path)
    config = fs_config(tmp_path, root=root)
    # The destination browser starts at rootle's cwd: run from the tmp
    # dir so the clone materializes hermetically (dest/org/repo).
    monkeypatch.chdir(tmp_path)
    out = run_headless(
        binary,
        # Select the org so the wizard lists its repos.
        "keys zzz\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"
        "keys :clone<cr>\n"
        "settle\n"
        "frame\n"
        "keys j<space>\n"  # uncheck beta
        "keys <tab><cr>\n"  # → destination (default: cwd)
        "frame\n"
        "keys <tab><cr>\n"  # → summary
        "frame\n"
        "keys <tab><cr>\n"  # → clone!
        "wait 1000\n"  # a real git clone runs on its own clock
        "settle\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    assert "clone — 1/3 repos" in f[0]
    assert "local/alpha" in f[0] and "local/beta" in f[0]
    assert "2/3 destination" in f[1]
    assert "3/3 summary" in f[2]
    assert "git clone" in f[2]
    assert "local/alpha" in f[2] and "local/beta" not in f[2]
    assert "cloned 1 repo" in f[3]
    cloned = tmp_path / "local" / "alpha"
    assert (cloned / ".git").is_dir(), "clone should materialize a git repo"


def test_yank_file_yields_blob_url(tmp_path, binary):
    """A file under the cursor yanks the FILE (blob) URL, not the dir."""
    clip = tmp_path / "clip.txt"
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys alpha\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"
        "keys l\n"  # into src/, main.rs selected
        "settle\n"
        "frame\n"
        "keys <space>y\n"
        "state\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
        env_extra={"ROOTLE_CLIPBOARD": str(clip)},
    )
    (state,) = states(out)
    f = frames(out)
    assert "fn render" in f[0]  # the blob preview landed
    assert "yanked" in f[1]
    assert "src/main.rs" in f[1]  # the FILE, not its dir
    url = state["yanks"][0]
    assert url.endswith("/alpha/src/main.rs#L1")  # preview line cursor
    assert clip.read_text().endswith("/alpha/src/main.rs#L1")


def test_delete_marked_orgs(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys zzz\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"
        "keys <space>d\n"  # nothing marked → honest no-op toast
        "frame\n"
        "keys hh\n"  # → orgs level
        "keys v\n"
        "keys <space>\n"  # VISUAL-mark the org
        "keys v\n"
        "keys <space>d\n"  # ␣d deletes it
        "settle\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    assert "no marked orgs" in f[0]
    assert "deleted 1 org" in f[1]
    assert "local/" not in f[1]  # gone from the orgs pane


def test_org_mark_fans_out_to_all_repos(tmp_path, binary):
    """Marking the org expands to ALL its repos (worker-resolved)."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys zzz\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"
        "keys hh\n"  # → orgs level
        "keys v\n"
        "keys <space>\n"  # mark the ORG
        "keys v\n"
        "keys :clone<cr>\n"
        "settle\n"  # the worker expands the org into its repos
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    (f,) = frames(out)
    assert "clone — 1/3 repos" in f
    assert "local/alpha" in f and "local/beta" in f
    assert "● local/alpha" in f  # dot markers, not [x]
