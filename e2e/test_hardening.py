"""E2E for plans/0008 (remote-provider hardening): a hung backend call
fails at the read deadline instead of wedging the UI, and a dead child
is respawned with backoff — the session recovers without a relaunch."""

import json
import time
from pathlib import Path

from conftest import Tui
from test_wiring import launch


def write_provider(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(0o755)


def _binary():
    from tui import build

    return build()


SLOW_PROVIDER = r"""
import sys, json, time
for line in sys.stdin:
    msg = json.loads(line)
    if msg.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"],
                          "result": {"protocol": 1, "name": "slow"}}), flush=True)
        continue
    time.sleep(3600)  # every real call hangs forever
"""

# Answers the handshake and the repo search, then dies on the next
# call — once, ever (marker file). The respawned generation answers
# tree + blob so the browse completes.
FLAKEY_PROVIDER = r"""
import sys, json, os, base64
marker = sys.argv[1]
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method", "")
    mid = msg.get("id")
    if method == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": mid,
                          "result": {"protocol": 1, "name": "flakey"}}), flush=True)
    elif method == "search/repos":
        print(json.dumps({"jsonrpc": "2.0", "id": mid,
                          "result": {"items": [{"full_name": "local/alpha"}]}}), flush=True)
    elif not os.path.exists(marker):
        open(marker, "w").close()
        sys.exit(0)  # die once: the transport must recover by itself
    elif method == "repo/tree":
        print(json.dumps({"jsonrpc": "2.0", "id": mid, "result": {
            "entries": [{"path": "README.md", "type": "blob", "sha": "abc123", "size": 9}],
            "truncated": False, "branch": "main"}}), flush=True)
    elif method == "repo/blob":
        data = base64.b64encode(b"# alpha\n").decode()
        print(json.dumps({"jsonrpc": "2.0", "id": mid,
                          "result": {"bytes_b64": data}}), flush=True)
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": mid, "result": {}}), flush=True)
"""


def test_hung_call_times_out_and_ui_stays_responsive(tmp_path: Path) -> None:
    provider = tmp_path / "slow_provider.py"
    write_provider(provider, SLOW_PROVIDER)
    config = tmp_path / "provider.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\ntimeout_ms = 2000\n'
        f'command = ["python3", "{provider}"]\n'
    )
    tui = Tui(_binary(), cols=110, rows=30, args=["--config", str(config)]).start()
    try:
        tui.type_query("alpha")
        tui.key("ENTER")
        # The search hangs; ~2s later the deadline fires with a
        # timeout status — the app never wedges (expect polls the UI).
        tui.expect("provider timeout", timeout=10)
        # Still responsive: the popup cancels and the app quits cleanly.
        tui.key("ESC")
        tui.key("ESC")
        tui.send("q")
        deadline = time.monotonic() + 5
        while tui._proc.poll() is None and time.monotonic() < deadline:
            time.sleep(0.05)
        assert tui._proc.poll() is not None, "app should quit after a timed-out provider"
    finally:
        tui.stop()


def test_dead_child_respawns_and_session_recovers(tmp_path: Path) -> None:
    provider = tmp_path / "flakey_provider.py"
    write_provider(provider, FLAKEY_PROVIDER)
    marker = tmp_path / "died-once"
    config = tmp_path / "provider.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{provider}", "{marker}"]\n'
    )
    tui = Tui(_binary(), cols=110, rows=30, args=["--config", str(config)]).start()
    try:
        tui.type_query("alpha")
        tui.key("ENTER")
        tui.expect("local/alpha")
        tui.key("ENTER")  # open the repo — the child dies on this call
        tui.expect("provider closed its output")
        # Retry: ensure_alive respawns (1s backoff + handshake), the
        # new generation serves the tree, and the status line notes it.
        tui.key("ENTER")
        tui.expect("provider restarted", timeout=10)
        tui.expect("README.md", timeout=10)
        tui.expect("# alpha", timeout=10)  # blob preview landed too
    finally:
        tui.stop()
