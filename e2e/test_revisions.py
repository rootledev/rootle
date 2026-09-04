"""Headless-tier e2e for protocol v1.5 (plans/0016 M1): the revision
switcher, file history, blame lens, open-at-commit, and the permalink
yank — against a real git worktree served by the fs reference adapter.
plans/0023: scripted keys + frame/state dumps via `rootle --headless`,
no PTY."""

from __future__ import annotations

from conftest import make_git_root
from headless import frames, fs_config, run_headless, states

# Search proj, open its tree: lands browsing local/proj at the default
# ref, main.rs listed and loaded.
OPEN_PROJ = "keys proj\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"


def git_config(tmp_path) -> str:
    """The fs stdio provider over a real git worktree."""
    return fs_config(tmp_path, root=make_git_root(tmp_path))


def test_revision_switcher_swaps_the_tree(tmp_path, binary):
    config = git_config(tmp_path)
    out = run_headless(
        binary,
        OPEN_PROJ
        + "frame\n"  # the tree at the default ref
        + "keys <space>b\n"  # leader → revisions
        # No "loading revisions" expectation: a fast local provider
        # clears the transient inside one tick — assert the durable
        # branch list instead.
        + "settle\n"  # refs load from the worktree
        + "frame\n"
        + "keys k\n"  # main → feature (branches sort above the tag)
        + "frame\n"  # the crumb previews the switch
        + "state\n"
        + "keys <cr>\n"  # commit the switch
        + "settle\n"  # the tree reloads at feature
        + "keys l\n"  # into main.rs
        + "settle\n"  # the blob lands
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    s = states(out)
    assert "main.rs" in f[0]
    assert "feature" in f[1]
    assert "main" in f[1]
    assert "default" in f[1]  # the default-branch marker rides the row
    assert "@ feature" in f[2]  # the crumb previews the switch
    assert s[0]["refs_popup"] is True
    assert s[0]["ref"] == "feature"
    # The tree now serves feature: main.rs grew a println.
    assert 'println!("hi")' in f[3]
    assert "@ feature" in f[3]


def test_history_blame_and_open_at_commit(tmp_path, binary):
    config = git_config(tmp_path)
    out = run_headless(
        binary,
        OPEN_PROJ
        + "keys l\n"  # into the tree; cursor on main.rs
        + "settle\n"  # the blob gates the lens: preview needs the pane
        + "keys <space>p\n"  # leader → preview
        + "settle\n"
        + "state\n"
        + "keys h\n"  # history lens
        + "settle\n"
        + "state\n"
        + "frame\n"
        + "keys y\n"  # v1.5 honesty: yank the sha-anchored permalink
        + "state\n"
        + "frame\n"
        + "keys <cr>\n"  # open at the commit
        + "settle\n"  # BlobAtLoaded flips the view atomically
        + "state\n"
        + "frame\n"
        + "keys <esc>\n"  # restore the present
        + "settle\n"
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    s = states(out)
    assert s[0]["mode"] == "PREVIEW"
    assert s[1]["mode"] == "HISTORY"
    assert "initial main.rs" in f[0]
    assert "HISTORY" in f[0]
    # The permalink carries the commit sha as its ref — no line anchor.
    assert len(s[2]["yanks"]) == 1
    assert "#L" not in s[2]["yanks"][0]
    assert "yanked" in f[1]
    assert "@ " in f[1] or "#L" not in f[1]  # sha-anchored, no line
    assert s[3]["mode"] == "PREVIEW"
    assert "initial main.rs · Tarek" in f[2]  # the band's commit context
    assert " @ " in f[2]  # commit marker in the title
    # The at-commit view must stay syntax-highlighted — the footer
    # reads the language, not "text".
    assert "rust · " in f[2], f"at-commit view lost highlighting: {f[2]}"
    assert "fn main" in f[3]
    assert " @ " not in f[3], "the marker must not stick after restore"


def test_visual_select_copy_and_range_yank(tmp_path, binary):
    """vim V in the pane: lines select, Y copies them (ROOTLE_CLIPBOARD
    is honored even headless), y yanks a #L2-L4 range URL."""
    clip = tmp_path / "clipboard.txt"
    config = git_config(tmp_path)
    out = run_headless(
        binary,
        OPEN_PROJ
        + "keys <space>p\n"
        + "settle\n"
        + "keys v\n"  # anchor at line 1
        + "keys jj\n"  # select lines 1-3
        + "state\n"
        + "frame\n"
        + "keys Y\n"  # copy the lines
        + "state\n"
        + "frame\n"
        + "keys y\n"  # the URL anchors the range
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
        env_extra={"ROOTLE_CLIPBOARD": str(clip)},
    )
    f = frames(out)
    s = states(out)
    # The preview keeps the PREVIEW mode; the line-select readout is
    # pane UI text.
    assert s[0]["mode"] == "PREVIEW"
    assert "VISUAL 1-3" in f[0]
    assert "yanked" in f[2]
    assert "#L1-L3" in f[2], f"range fragment missing: {f[2]}"
    # The clipboard override writes the file even headless; the range
    # yank is the last write.
    assert clip.read_text().endswith("#L1-L3")


def test_blame_lens_marks_runs(tmp_path, binary):
    config = git_config(tmp_path)
    out = run_headless(
        binary,
        OPEN_PROJ
        + "keys l\n"
        + "settle\n"
        + "keys <space>p\n"
        + "settle\n"
        + "keys b\n"  # blame lens
        + "settle\n"  # git blame runs
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    (f,) = frames(out)
    # The margin appears with the author and a run-start sha.
    assert "Tarek" in f
    assert " │ " in f
