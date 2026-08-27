"""E2E for the provider seam (plans/0005): the full TUI running on an
external stdio provider — examples/providers/fs_provider.py serving a
temp directory. Proves the NDJSON-RPC protocol end to end, offline:
repo search, tree browsing, blob preview, and global-scope code search
all go through the child process."""

import subprocess
import time

from conftest import dismiss_launch_popup
from tui import Tui


def provider_pids(tui: Tui) -> list[str]:
    """fs provider children of THIS rootle instance — strays from other
    sessions never pollute the assertion."""
    out = subprocess.run(
        ["pgrep", "-P", str(tui._proc.pid), "-f", "fs_provider.py"],
        capture_output=True,
        text=True,
    ).stdout.split()
    return [p for p in out]


def test_fs_provider_search_to_tree_to_preview(provider_tui: Tui) -> None:
    tui = provider_tui
    # Search hits the provider over stdio; the repo opens, tree walks.
    tui.type_query("alpha")
    tui.key("ENTER")
    screen = tui.expect("local/alpha")
    assert "[repo]" in screen

    tui.key("ENTER")
    # The tree pane and blob content arrive from the provider over
    # separate async round trips — poll each string instead of
    # asserting on one captured frame (a PTY diff can tear mid-draw:
    # the frame that first shows the needle may not have the rest).
    tui.expect("README.md")
    tui.expect("src/")
    tui.expect("main.rs")

    # Preview is the blob fetched through the provider (content hash).
    tui.send("l")  # into src
    tui.expect("main.rs")
    tui.expect("fn render")
    tui.expect("rootle")


def test_fs_provider_grep_over_stdio(provider_tui: Tui) -> None:
    tui = provider_tui
    # Close the launch popup (waits for the close — an ESC that outruns
    # it lands in the freshly opened view instead).
    dismiss_launch_popup(tui)
    tui.send(" ")
    tui.send("g")
    screen = tui.expect("grep")
    assert "global" in screen

    tui.type_query("render")
    tui.key("ENTER")
    # Path rows render first; the badge and located preview lines fill
    # in when the lazy per-hit context lands — poll for each.
    tui.expect("alpha/src/main.rs")
    tui.expect("matches")  # folded match-count badge
    tui.expect("fn render() -> &'static str {")  # located region

def test_nested_repo_ids_are_opaque(provider_tui: Tui) -> None:
    """Multi-slash repo ids (GitLab's group/subgroup/project shape)
    flow through search, the browser, and the preview untouched —
    plans/0009 R2: the UI never parses repo strings."""
    tui = provider_tui
    tui.type_query("deep")
    tui.key("ENTER")
    tui.expect("[repo]")
    tui.expect("local/nested/sub/deep")

    tui.key("ENTER")
    tui.expect("lib.rs")
    # Preview is the blob from the nested repo (content hash).
    tui.send("l")
    tui.expect("deep_fn")
    tui.expect("42")


def test_provider_process_is_spawned_and_child(provider_tui: Tui) -> None:
    """The stdio child must exist while the app runs and die with it
    (it may take a beat to notice stdin EOF after the app exits)."""
    tui = provider_tui
    tui.expect("search fs")
    assert provider_pids(tui), "provider child should be running"
    tui.stop()
    # rootle exits gracefully on SIGTERM → App drop kills the child
    # deterministically (kill + reap in StdioProvider::drop).
    assert not provider_pids(tui), "provider child should die with the app"
