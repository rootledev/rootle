"""E2E for the provider seam (plans/0005): the full TUI running on an
external stdio provider — examples/providers/fs_provider.py serving a
temp directory. Proves the NDJSON-RPC protocol end to end, offline:
repo search, tree browsing, blob preview, and global-scope code search
all go through the child process."""

import subprocess
import time

from tui import Tui


def provider_pids() -> list[str]:
    """fs provider children (python3), never the test machinery."""
    out = subprocess.run(
        ["pgrep", "-f", "fs_provider.py"], capture_output=True, text=True
    ).stdout.split()
    pids = []
    for p in out:
        try:
            cmdline = open(f"/proc/{p}/cmdline").read()
        except OSError:
            continue
        if cmdline.startswith("python3"):
            pids.append(p)
    return pids


def test_fs_provider_search_to_tree_to_preview(provider_tui: Tui) -> None:
    tui = provider_tui
    # Search hits the provider over stdio; the repo opens, tree walks.
    tui.type_query("alpha")
    tui.key("ENTER")
    screen = tui.expect("local/alpha")
    assert "[repo]" in screen

    tui.key("ENTER")
    screen = tui.expect("README.md")
    assert "src/" in screen
    tui.expect("main.rs")

    # Preview is the blob fetched through the provider (content hash).
    tui.send("l")  # into src
    tui.expect("main.rs")
    screen = tui.expect("fn render")
    assert "ghx" in screen


def test_fs_provider_grep_over_stdio(provider_tui: Tui) -> None:
    tui = provider_tui
    # Close the launch popup: no repo open → global scope over the
    # provider's code search.
    tui.key("ESC")
    tui.key("ESC")
    tui.send(" ")
    tui.send("g")
    screen = tui.expect("grep")
    assert "global" in screen

    tui.type_query("render")
    tui.key("ENTER")
    screen = tui.expect("alpha/src/main.rs")
    assert "matches" in screen  # folded match-count badge
    assert "fn render() -> &'static str {" in screen  # located region


def test_provider_process_is_spawned_and_child(provider_tui: Tui) -> None:
    """The stdio child must exist while the app runs and die with it
    (it may take a beat to notice stdin EOF after the app exits)."""
    tui = provider_tui
    tui.expect("search github")
    assert provider_pids(), "provider child should be running"
    tui.stop()
    deadline = time.monotonic() + 3.0
    while provider_pids() and time.monotonic() < deadline:
        time.sleep(0.1)
    assert not provider_pids(), "provider child should die with the app"
