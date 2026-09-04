"""Search-pane parity e2e (0019), headless tier: the expanded file pane
speaks the preview-submode grammar — `y` yanks the cursor-anchored remote
URL, `:N` jumps by line, `b` runs the blame lens with run margins. Drives
the real binary via `rootle --headless -` on the fs stdio provider over a
git fixture (blame needs history), fully offline. Yanks are asserted from
the `state` step's recorded clipboard, not a toast race."""

from pathlib import Path

from conftest import make_git_root
from headless import frames, fs_config, run_headless, states


def test_expanded_pane_yank_goto_and_blame(tmp_path: Path, binary) -> None:
    config = fs_config(tmp_path, root=make_git_root(tmp_path))
    out = run_headless(
        binary,
        # Repo search -> open the fixture repo.
        "keys proj\n"
        "keys <cr>\n"
        "settle\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n"
        # Leader g — global grep over the open repo.
        "keys <space>\n"
        "keys g\n"
        "settle\n"
        "keys main\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n"
        "keys <cr>\n"  # expand the hit
        "settle\n"
        "frame\n"
        # y — the cursor line's URL is yanked.
        "keys y\n"
        "state\n"
        # :2 — command line opens from the pane; the jump moves the
        # cursor to line 2 of the fixture's main.rs.
        "keys :\n"
        "keys 2\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n"
        # b — blame lens: run margins carry sha + author right of the
        # gutter ("fe6823b Tarek" — the band reads "fe6823b · … · Tarek",
        # so this shape only matches margins).
        "keys b\n"
        "settle\n"
        "frame\n"
        # b again clears it.
        "keys b\n"
        "settle\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=170,
    )
    tree, results, expanded, goto, blame, cleared = frames(out)
    (yank,) = states(out)

    assert "main.rs" in tree
    assert "main.rs" in results
    assert "fn main" in expanded and "main.rs" in expanded

    assert (yank["status"] or "").startswith("yanked")
    assert "main.rs" in yank["yanks"][0]

    # The jump moved the cursor: readout 2/5, file still under the pane.
    assert "fn main" in goto
    assert "2/5" in goto

    assert "fe6823b Tarek" in blame
    assert "fe6823b Tarek" not in cleared


def test_preview_band_shows_last_commit(tmp_path: Path, binary) -> None:
    """0019 polish: previewing a file dresses the header band with the
    file's last commit (sha · subject · author · date), ambient and
    cached — github.com's file header, in the terminal."""
    config = fs_config(tmp_path, root=make_git_root(tmp_path))
    out = run_headless(
        binary,
        "keys proj\n"
        "keys <cr>\n"
        "settle\n"
        "keys <cr>\n"  # open the repo
        "settle\n"  # the ambient git-log fetch dresses the band
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=170,
    )
    (frame,) = frames(out)
    assert "main.rs" in frame
    assert "initial main.rs" in frame  # subject
    assert "Tarek" in frame and "2026-08-01" in frame, frame


def test_repo_popup_yank_no_dead_keys(tmp_path: Path, binary) -> None:
    """0021 M3 hygiene: `y` in the repo search popup yanks the selected
    repo's URL instead of dying silently."""
    config = fs_config(tmp_path, root=make_git_root(tmp_path))
    out = run_headless(
        binary,
        "keys proj\n"
        "keys <cr>\n"  # submit — results focus
        "settle\n"
        "frame\n"
        "keys y\n"
        "state\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    results, after = frames(out)
    (state,) = states(out)
    assert "local/proj" in results
    assert (state["status"] or "").startswith("yanked")
    assert state["yanks"][0].endswith("/proj")
    # The popup survives the yank — no dead key, no closed view.
    assert "local/proj" in after
