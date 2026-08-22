"""E2E for the v0.3/v0.4 wiring: yank to clipboard (ROOTLE_CLIPBOARD file
override — no clipboard in CI), settings write-back, and the clone
wizard running real `git clone` through a provider-supplied clone URL
(the fs provider's repos are real git repos here)."""

import subprocess
from pathlib import Path

import pytest

from conftest import FS_PROVIDER, Tui, dismiss_launch_popup, make_fs_root
from tui import build


@pytest.fixture
def git_root(tmp_path: Path) -> Path:
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


def launch(tmp_path: Path, root: Path, env_extra: dict[str, str] | None = None) -> Tui:
    config = tmp_path / "provider.toml"
    config.write_text(
        f'[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    return Tui(
        build(), cols=110, rows=30, args=["--config", str(config)], env_extra=env_extra
    ).start()


def test_yank_writes_clipboard(tmp_path: Path) -> None:
    clip = tmp_path / "clip.txt"
    root = make_fs_root(tmp_path)
    tui = launch(tmp_path, root, {"ROOTLE_CLIPBOARD": str(clip)})
    try:
        tui.type_query("alpha")
        tui.key("ENTER")
        tui.expect("local/alpha")
        tui.key("ENTER")
        tui.expect("README.md")

        tui.send(" ")
        tui.send("y")
        screen = tui.expect("yanked")
        assert "file://" in screen  # provider-supplied URL
        assert clip.read_text().endswith("/alpha")  # repo root URL yanked
    finally:
        tui.stop()


def test_settings_write_back_and_toast(tmp_path: Path) -> None:
    root = make_fs_root(tmp_path)
    tui = launch(tmp_path, root)
    try:
        dismiss_launch_popup(tui)
        tui.send(":")
        tui.type_query("settings")
        tui.key("ENTER")
        tui.expect("settings")

        # editor tab is first; land on cache.max_mb: Tab×2, j×0 (only
        # field), Enter to edit, retype the value, Enter commits.
        tui.key("TAB")  # → theme
        tui.key("TAB")  # → cache
        tui.key("ENTER")  # edit max_mb (prefilled 512)
        for _ in range(3):
            tui.key("BACKSPACE")
        tui.type_query("128")
        tui.key("ENTER")  # commit field
        tui.key("ESC")  # close popup → dirty → ApplySettings
        tui.expect("settings saved")

        # The hermetic XDG config got the written section.
        config = Path(tui._home.name) / "config" / "rootle" / "config.toml"
        assert "max_mb = 128" in config.read_text()
    finally:
        tui.stop()


def test_settings_theme_radio_selects_and_saves(tmp_path: Path) -> None:
    root = make_fs_root(tmp_path)
    # A second theme in the hermetic XDG dir; save() also writes there.
    xdg = tmp_path / "xdg"
    themes = xdg / "rootle" / "themes"
    themes.mkdir(parents=True)
    (themes / "gruvbox-dark.toml").write_text('[semantic]\nborder_focused = "#b8bb26"\n')
    tui = launch(tmp_path, root, env_extra={"XDG_CONFIG_HOME": str(xdg)})
    try:
        dismiss_launch_popup(tui)
        tui.send(":")
        tui.type_query("settings")
        tui.key("ENTER")
        tui.expect("editor")

        tui.key("TAB")  # → theme section: radio list of palettes
        screen = tui.expect("gruvbox-dark")
        assert "\u25cf catppuccin-mocha" in screen  # current theme carries the dot

        # Alphabetical: catppuccin-latte → catppuccin-mocha → dracula →
        # github-light → gruvbox-dark.
        for _ in range(4):
            tui.send("j")
        tui.send(" ")  # select gruvbox-dark → live preview + unsaved chip
        screen = tui.expect("unsaved")
        assert "\u25cf gruvbox-dark" in screen
        assert "\u25cb catppuccin-mocha" in screen

        tui.key("ESC")  # dirty → ApplySettings
        tui.expect("settings saved")
        assert 'name = "gruvbox-dark"' in (xdg / "rootle" / "config.toml").read_text()
    finally:
        tui.stop()


def test_clone_wizard_runs_git_clone(tmp_path: Path, git_root: Path) -> None:
    root = git_root
    tui = launch(tmp_path, root)
    try:
        # Select the org so the wizard lists its repos.
        tui.type_query("zzz")  # no repo match → org listed
        tui.key("ENTER")
        tui.expect("local")
        tui.key("ENTER")
        tui.expect("beta/")  # org repos level loaded

        tui.send(":")
        tui.type_query("clone")
        tui.key("ENTER")
        screen = tui.expect("clone — 1/3 repos")
        assert "local/alpha" in screen and "local/beta" in screen

        # Uncheck beta; walk to the destination screen and set dest.
        tui.send("j")
        tui.send(" ")
        tui.key("TAB")  # → buttons (on next)
        tui.key("ENTER")  # → destination
        tui.expect("2/3 destination")
        # Dest browser starts at rootle's cwd (the e2e dir); navigate to
        # the tmp dir via .. then by name — simpler: type-free nav is
        # fragile; instead walk up to / and down. Use the fact that
        # dest starts at cwd: go up until /, impossible to assert.
        # Pragmatic: accept the default dest (cwd) — clone into a
        # subdir there instead. So keep default and finish.
        tui.key("TAB")
        tui.key("ENTER")  # → summary
        screen = tui.expect("3/3 summary")
        assert "git clone" in screen
        assert "local/alpha" in screen and "local/beta" not in screen

        tui.key("TAB")  # → clone!
        tui.key("ENTER")
        screen = tui.expect("cloned 1 repo")
        print(screen)
        # Cloned into <cwd>/local/alpha — dest/org/repo, org level
        # prevents same-name collisions.
        cloned = Path.cwd() / "local" / "alpha"
        assert (cloned / ".git").is_dir(), "clone should materialize a git repo"
    finally:
        tui.stop()
        subprocess.run(["rm", "-rf", str(Path.cwd() / "local")], check=False)


def test_yank_file_yields_blob_url(tmp_path: Path) -> None:
    """A file under the cursor yanks the FILE (blob) URL, not the dir."""
    clip = tmp_path / "clip.txt"
    root = make_fs_root(tmp_path)
    tui = launch(tmp_path, root, {"ROOTLE_CLIPBOARD": str(clip)})
    try:
        tui.type_query("alpha")
        tui.key("ENTER")
        tui.expect("local/alpha")
        tui.key("ENTER")
        tui.expect("main.rs")
        tui.send("l")  # into src/, main.rs selected
        tui.expect("fn render")

        tui.send(" ")
        tui.send("y")
        screen = tui.expect("yanked")
        assert "src/main.rs" in screen  # the FILE, not its dir
        assert clip.read_text().endswith("/alpha/src/main.rs#L1")  # preview line cursor
    finally:
        tui.stop()


def test_delete_marked_orgs(tmp_path: Path) -> None:
    root = make_fs_root(tmp_path)
    tui = launch(tmp_path, root)
    try:
        # Load the org's repos, mark nothing, ␣d → honest no-op toast.
        tui.type_query("zzz")
        tui.key("ENTER")
        tui.expect("local")
        tui.key("ENTER")
        tui.expect("beta/")
        tui.send(" ")
        tui.send("d")
        tui.expect("no marked orgs")

        # VISUAL-mark the org at the orgs level, ␣d deletes it.
        tui.send("h")  # → repos level
        tui.send("h")  # → orgs level
        tui.send("v")
        tui.send(" ")  # mark local
        tui.send("v")
        tui.send(" ")
        tui.send("d")
        screen = tui.expect("deleted 1 org")
        assert "local/" not in screen  # gone from the orgs pane
    finally:
        tui.stop()


def test_org_mark_fans_out_to_all_repos(tmp_path: Path) -> None:
    """Marking the org expands to ALL its repos (worker-resolved)."""
    root = make_fs_root(tmp_path)
    tui = launch(tmp_path, root)
    try:
        tui.type_query("zzz")
        tui.key("ENTER")
        tui.expect("local")
        tui.key("ENTER")
        tui.expect("beta/")

        tui.send("h")  # → repos level
        tui.send("h")  # → orgs level
        tui.send("v")
        tui.send(" ")  # mark the ORG
        tui.send("v")
        tui.send(":")
        tui.type_query("clone")
        tui.key("ENTER")
        # Worker expanded the org: wizard opens with BOTH repos.
        screen = tui.expect("clone — 1/3 repos", timeout=8)
        assert "local/alpha" in screen and "local/beta" in screen
        assert "● local/alpha" in screen  # dot markers, not [x]
    finally:
        tui.stop()
