"""E2E for protocol v1.5 (plans/0016 M1): the revision switcher, file
history, blame lens, open-at-commit, and the permalink yank — against
a real git worktree served by the fs reference adapter."""

from pathlib import Path

import pytest
from conftest import FS_PROVIDER, make_git_root
from tui import Tui


@pytest.fixture
def git_tui(tmp_path, binary):
    root = make_git_root(tmp_path)
    config = tmp_path / "p.toml"
    config.write_text(
        "[provider]\n"
        'kind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    t = Tui(binary, cols=110, rows=30, args=["--config", str(config)]).start()
    yield t
    t.stop()


def open_proj(tui: Tui) -> None:
    tui.type_query("proj")
    tui.key("ENTER")
    tui.expect("local/proj")
    tui.key("ENTER")
    tui.expect("main.rs")


def test_revision_switcher_swaps_the_tree(git_tui: Tui) -> None:
    tui = git_tui
    open_proj(tui)
    tui.send(" ")  # leader
    tui.send("b")  # revisions
    tui.expect("loading revisions")
    # branches + the default marker land
    tui.expect("feature")
    tui.expect("main")
    assert "default" in tui.screen()
    tui.send("k")  # main → feature (branches sort above the tag)
    tui.expect("@ feature")  # crumb previews the switch
    tui.key("ENTER")  # commit
    # The tree now serves feature: main.rs grew a println.
    tui.send("l")
    tui.expect('println!("hi")')
    assert "@ feature" in tui.screen()


def test_history_blame_and_open_at_commit(git_tui: Tui) -> None:
    tui = git_tui
    open_proj(tui)
    tui.send("l")  # into the tree; cursor on main.rs
    tui.send(" ")
    tui.send("p")
    tui.expect("PREVIEW")
    tui.send("h")  # history lens
    screen = tui.expect("initial main.rs")
    assert "HISTORY" in screen
    # v1.5 honesty: `y` yanks the permalink anchored to the commit sha.
    tui.send("y")
    screen = tui.expect("yanked")
    assert "@ " in screen or "#L" not in screen  # sha-anchored, no line
    tui.key("ENTER")  # open at the commit
    screen = tui.expect("fn main() {}")  # the OLD content
    assert "PREVIEW" in screen
    assert " @ " in screen  # commit marker in the title
    # The demo-caught regression: the at-commit view must still be
    # syntax-highlighted — the footer reads the language, not "text".
    assert "rust · " in screen, f"at-commit view lost highlighting: {screen}"
    tui.key("ESC")  # restore the present
    tui.expect('println!("hi")' if "feature" in tui.screen() else "fn main")
    assert " @ " not in tui.screen(), "the marker must not stick after restore"


def test_blame_lens_marks_runs(git_tui: Tui) -> None:
    tui = git_tui
    open_proj(tui)
    tui.send("l")
    tui.send(" ")
    tui.send("p")
    tui.expect("PREVIEW")
    tui.send("b")
    # The margin appears with the author and a run-start sha.
    screen = tui.expect("Tarek")
    assert " │ " in screen
