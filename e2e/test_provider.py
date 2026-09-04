"""E2E for the provider seam (plans/0005), headless tier: the full app
running on an external stdio provider — examples/providers/fs_provider.py
serving a temp directory via `rootle --headless -`. Proves the NDJSON-RPC
protocol end to end, offline: repo search, tree browsing, blob preview,
global-scope code search, and the child process lifecycle all go through
the child process."""

from __future__ import annotations

import subprocess
import time
from pathlib import Path

from headless import frames, fs_config, run_headless, states
from tui import hermetic_env


def test_fs_provider_search_to_tree_to_preview(tmp_path, binary):
    """Search hits the provider over stdio; the repo opens, the tree
    walks, and the preview is the blob fetched through the provider
    (content hash) — each in its own settled frame."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys alpha\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n"  # results row: [repo] local/alpha
        "keys <cr>\n"
        "settle\n"
        "frame\n"  # tree + dir preview
        "keys l\n"  # into src
        "settle\n"
        "frame\n"  # blob preview
        "state\n",
        "--config",
        str(config),
        home=tmp_path / "home",
    )
    results, tree, blob = frames(out)
    assert "[repo]" in results and "local/alpha" in results

    # The tree pane and blob content arrive from the provider over
    # separate async round trips — a settled frame carries them all.
    assert "README.md" in tree
    assert "src/" in tree
    assert "main.rs" in tree  # the selected src/ dir previews its child

    assert "fn render" in blob
    assert "rootle" in blob
    (state,) = states(out)
    assert state["provider"] == "stdio:fs"


def test_fs_provider_grep_over_stdio(tmp_path, binary):
    """Global code search rides the provider: query in, path rows and
    located preview lines out — the badge and context fill in once the
    lazy per-hit fetch lands."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys <esc><esc>\n"  # close the launch popup (INSERT→NORMAL→close)
        "keys <space>\n"
        "keys g\n"  # leader g — the global grep view, scope: global
        "settle\n"
        "frame\n"
        "keys render\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
    )
    scope, results = frames(out)
    assert "grep" in scope
    assert "global" in scope  # scope chip: no repo open
    assert "alpha/src/main.rs" in results
    assert "matches" in results  # folded match-count badge
    assert "fn render() -> &'static str {" in results  # located region


def test_nested_repo_ids_are_opaque(tmp_path, binary):
    """Multi-slash repo ids (GitLab's group/subgroup/project shape) flow
    through search, the browser, and the preview untouched — plans/0009
    R2: the UI never parses repo strings."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        "keys deep\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n"
        "keys <cr>\n"  # open the selected repo
        "settle\n"
        "frame\n"
        "keys l\n"  # into lib.rs's preview
        "settle\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
    )
    results, tree, preview = frames(out)
    assert "[repo]" in results
    assert "local/nested/sub/deep" in results

    assert "lib.rs" in tree
    # Preview is the blob from the nested repo (content hash).
    assert "deep_fn" in preview
    assert "42" in preview


def provider_pids(pid: int) -> list[str]:
    """fs provider children of THIS rootle instance — strays from other
    sessions never pollute the assertion."""
    out = subprocess.run(
        ["pgrep", "-P", str(pid), "-f", "fs_provider.py"],
        capture_output=True,
        text=True,
    ).stdout.split()
    return [p for p in out]


def test_provider_process_is_spawned_and_child(tmp_path, binary):
    """The stdio child must exist while the app runs and die with it
    (it may take a beat to notice stdin EOF after the app exits). The
    headless driver runs the same App — same spawn, same Drop — so the
    lifecycle is observable without a terminal."""
    config = fs_config(tmp_path)
    home = tmp_path / "home"
    home.mkdir(parents=True)
    env = hermetic_env(home)
    env["ROOTLE_HEADLESS_COLS"] = "110"
    env["ROOTLE_HEADLESS_ROWS"] = "30"
    proc = subprocess.Popen(
        [str(binary), "--headless", "-", "--config", str(config)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    assert proc.stdin is not None
    proc.stdin.write(b"wait 3000\n")  # keep the app alive to observe the child
    proc.stdin.flush()
    proc.stdin.close()
    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and not provider_pids(proc.pid):
            time.sleep(0.1)
        assert provider_pids(proc.pid), "provider child should be running"
        assert proc.wait(timeout=30) == 0
    finally:
        proc.kill()
    # rootle exits gracefully at script end → App drop kills the child
    # deterministically (kill + reap in StdioProvider::drop); allow a
    # beat for the corpse to clear the process table.
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline and provider_pids(proc.pid):
        time.sleep(0.1)
    assert not provider_pids(proc.pid), "provider child should die with the app"
