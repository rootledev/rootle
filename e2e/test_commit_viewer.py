"""Headless-tier e2e for the commit viewer (plans/0028): history `d`
dives into commit detail, Enter opens the file delta, the Esc ladder
unwinds — against a real git worktree served by the fs reference
adapter (protocol v1.6 `repo/commit`)."""

from __future__ import annotations

from conftest import make_git_root
from headless import fs_config, frames, run_headless, states

OPEN_PROJ = "keys proj\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"


def git_config(tmp_path) -> str:
    return fs_config(tmp_path, root=make_git_root(tmp_path))


def test_commit_viewer_dive_detail_and_delta(tmp_path, binary):
    config = git_config(tmp_path)
    out = run_headless(
        binary,
        OPEN_PROJ
        + "keys l\n"  # into the tree; cursor on main.rs
        + "settle\n"  # the blob gates the lens
        + "keys <space>p\n"  # preview submode
        + "settle\n"
        + "keys h\n"  # history lens
        + "settle\n"
        + "frame\n"
        + "keys d\n"  # dive into the commit
        + "settle\n"  # repo/commit lands from the worktree
        + "state\n"
        + "frame\n"
        + "keys <cr>\n"  # open the file delta
        + "settle\n"
        + "state\n"
        + "frame\n"
        + "keys <esc>\n"  # delta → detail
        + "settle\n"
        + "frame\n"
        + "keys <esc>\n"  # detail → history
        + "settle\n"
        + "state\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    s = states(out)
    assert "HISTORY" in f[0]
    assert "initial main.rs" in f[0]

    # The detail surface: header band (author/date), message, files.
    assert s[0]["mode"] == "COMMIT"
    assert s[0]["surface"]["sha"]
    assert s[0]["surface"]["delta"] is None
    assert "Tarek" in f[1]
    assert "2026-08-01" in f[1]
    assert "main.rs" in f[1]
    assert "files (1)" in f[1]

    # The delta: the root commit's hunk — all additions, old side blank.
    assert s[1]["surface"]["delta"] == 0
    assert "@@ -0,0 +1,5 @@" in f[2]
    assert "fn main()" in f[2]
    assert "1/1" in f[2]

    # The Esc ladder: back to the file list, then to history.
    assert "files (1)" in f[3]
    assert s[2]["mode"] == "HISTORY"
    assert s[2]["surface"] is None
