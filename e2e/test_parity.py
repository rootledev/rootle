"""Search-pane parity e2e (0019): the expanded file pane speaks the
preview-submode grammar — `y` yanks the cursor-anchored remote URL,
`:N` jumps by line, `b` runs the blame lens with run margins. Drives
the real binary on the fs stdio provider over a git fixture (blame
needs history), fully offline."""

from pathlib import Path

from conftest import FS_PROVIDER, make_git_root
from tui import Tui, build


def test_expanded_pane_yank_goto_and_blame(tmp_path: Path) -> None:
    root = make_git_root(tmp_path)
    config = tmp_path / "p.toml"
    config.write_text(
        f'[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    tui = Tui(build(), cols=170, rows=30, args=["--config", str(config)]).start()
    try:
        # Repo search -> open the fixture repo.
        tui.type_query("proj")
        tui.key("ENTER")
        tui.expect("local/proj")
        tui.key("ENTER")  # open the selected repo
        tui.expect("main.rs")
        tui.send(" ", settle=0.4)
        tui.expect("grep")
        tui.send("g", settle=0.3)
        tui.expect("query")
        tui.type_query("main")
        tui.key("ENTER")
        tui.expect("main.rs")
        tui.key("ENTER")  # expand the hit
        screen = tui.expect("fn main")
        assert "main.rs" in screen, screen

        # y — the cursor line's URL lands on the status line.
        tui.send("y")
        screen = tui.expect("yanked")
        assert "main.rs" in screen, screen

        # :2 — command line opens from the pane; the jump moves the
        # cursor (line 2 of the fixture's main.rs).
        tui.send(":")
        tui.type_query("2")
        tui.key("ENTER")
        tui.expect("fn main")

        # b — blame lens: run margins carry sha + author right of the
        # gutter ("Tarek │" — the band's author reads "Tarek · <date>",
        # so this shape only matches margins).
        tui.send("b")
        tui.expect("fe6823b Tarek")

        # b again clears it.
        tui.send("b")
        tui.expect_gone("fe6823b Tarek")
    finally:
        tui.stop()


def test_preview_band_shows_last_commit(tmp_path: Path) -> None:
    """0019 polish: previewing a file dresses the header band with the
    file's last commit (sha · subject · author · date), ambient and
    cached — github.com's file header, in the terminal."""
    root = make_git_root(tmp_path)
    config = tmp_path / "p.toml"
    config.write_text(
        f'[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    tui = Tui(build(), cols=170, rows=30, args=["--config", str(config)]).start()
    try:
        tui.type_query("proj")
        tui.key("ENTER")
        tui.expect("local/proj")
        tui.key("ENTER")  # open the repo
        tui.expect("main.rs")
        # The band dresses with the file's last commit once the ambient
        # fetch lands (fs provider: git log -1 main.rs).
        screen = tui.expect("initial main.rs")
        assert "Tarek" in screen and "2026-08-01" in screen, screen
    finally:
        tui.stop()


def test_repo_popup_yank_no_dead_keys(tmp_path: Path) -> None:
    """0021 M3 hygiene: `y` in the repo search popup yanks the selected
    repo's URL instead of dying silently."""
    root = make_git_root(tmp_path)
    config = tmp_path / "p.toml"
    config.write_text(
        f'[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    tui = Tui(build(), cols=110, rows=30, args=["--config", str(config)]).start()
    try:
        tui.type_query("proj")
        tui.key("ENTER")   # submit — results focus
        tui.expect("local/proj")
        tui.send("y")
        screen = tui.expect("yanked")
        assert "local/proj" in screen, screen
    finally:
        tui.stop()
