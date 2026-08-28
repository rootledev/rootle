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
    tui = Tui(build(), cols=110, rows=30, args=["--config", str(config)]).start()
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

        # b — blame lens: run margins carry the fixture's author.
        tui.send("b")
        tui.expect("Tarek")

        # b again clears it.
        tui.send("b")
        tui.expect_gone("Tarek")
    finally:
        tui.stop()
