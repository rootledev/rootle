"""Headless-tier e2e for plans/0008 (remote-provider hardening): a
hung backend call fails at the read deadline instead of wedging the
UI, and a dead child is respawned with backoff — the session recovers
without a relaunch. plans/0023: scripted keys + frame/state dumps; the
deadline and the respawn backoff are ridden out with `wait`."""

from __future__ import annotations

from pathlib import Path

from headless import frames, run_headless, states


def write_provider(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(0o755)


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


def test_hung_call_times_out_and_ui_stays_responsive(tmp_path, binary):
    provider = tmp_path / "slow_provider.py"
    write_provider(provider, SLOW_PROVIDER)
    config = tmp_path / "provider.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\ntimeout_ms = 2000\n'
        f'command = ["python3", "{provider}"]\n'
    )
    out = run_headless(
        binary,
        "keys alpha\n"
        "keys <cr>\n"
        "wait 3000\n"  # ride past the 2s read deadline
        "frame\n"
        "state\n"  # the popup still holds the error block
        "keys <esc><esc>\n"  # still responsive: the popup cancels
        "state\n"
        "keys q\n",  # …and the app quits — the run returning is the proof
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    s = states(out)
    # The deadline fired: the popup's results block carries the error
    # — the UI never wedged (the run itself returning is the proof).
    assert "provider timeout" in f[0]
    assert s[0]["popup"] is True
    # Keys still process after the deadline: the popup closed cleanly.
    assert s[1]["popup"] is False
    assert s[1]["mode"] == "BROWSE"


def test_dead_child_respawns_and_session_recovers(tmp_path, binary):
    provider = tmp_path / "flakey_provider.py"
    write_provider(provider, FLAKEY_PROVIDER)
    marker = tmp_path / "died-once"
    config = tmp_path / "provider.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{provider}", "{marker}"]\n'
    )
    out = run_headless(
        binary,
        "keys alpha\n"
        "keys <cr>\n"
        "settle\n"  # results land: initialize + search/repos answered
        "frame\n"
        "keys <cr>\n"  # open the repo — the child dies on this call
        "settle\n"  # the closed-output error surfaces
        "frame\n"
        "keys <cr>\n"  # retry: ensure_alive respawns (1s backoff + handshake)
        "wait 2500\n"  # the backoff gap outlives settle's quiet window
        "settle\n"  # the new generation serves tree + blob
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    assert "local/alpha" in f[0]
    assert "provider closed its output" in f[1]
    # The status line notes the respawn; the recovered tree and blob
    # preview land on the same frame.
    assert "provider restarted" in out
    assert "README.md" in f[2]
    assert "# alpha" in f[2]  # blob preview landed too
