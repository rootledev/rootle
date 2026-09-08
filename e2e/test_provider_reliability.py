"""Accepted readiness, owner provenance and durable failures through a real child."""

import json
import time
from pathlib import Path

import pytest

from headless import frames, run_headless, states
from test_diagnostics import trace_records
from tui import Tui


PROVIDER = r'''
import base64, json, sys
from pathlib import Path
log, scenario, marker = map(str, sys.argv[1:4])
held = []
trees = 0

def send(value):
    print(json.dumps(value), flush=True)

def result(mid, value):
    send({"jsonrpc": "2.0", "id": mid, "result": value})

def fail(mid, kind="auth", message="SEARCH_TOOL_REQUIRED: grok unavailable outside Spaces\x1b[2J", retry=None):
    data = {"kind": kind}
    if retry is not None:
        data["retry_after_s"] = retry
    send({"jsonrpc": "2.0", "id": mid, "error": {"code": 1, "message": message, "data": data}})

def tree(name=None, revision="main"):
    entries = [{"path": name, "type": "blob", "sha": "blob"}] if name else [
        {"path": "README.md", "type": "blob", "sha": "blob"},
        {"path": "src", "type": "tree", "sha": "dir"},
        {"path": "src/lib.rs", "type": "blob", "sha": "blob"},
    ]
    return {"entries": entries, "truncated": False, "branch": revision}

for line in sys.stdin:
    message = json.loads(line)
    method, mid = message.get("method"), message.get("id")
    with open(log, "a") as output:
        output.write(json.dumps(message) + "\n")
    if mid is None:
        continue
    params = message.get("params", {})
    if method == "initialize":
        result(mid, {"protocol": 1, "name": "reliability", "capabilities": {
            "orgs": True, "code_search": scenario != "unsupported", "file_search": False,
        }})
    elif method == "org/repos":
        fail(mid, "not_found", "OWNER_LIST_NOT_FOUND")
    elif method == "search/repos":
        result(mid, {"items": [{"full_name": "personal/project"}]})
    elif method == "repo/tree":
        trees += 1
        if scenario == "reorder" and trees in (2, 3):
            held.append(mid)
            Path(marker).write_text(str(trees))
        elif scenario == "reorder" and trees == 4:
            result(mid, tree("LIVE_NEW.rs"))
            result(held[1], tree("OBSOLETE.rs"))
            fail(held[0], "provider", "OBSOLETE_FAILURE")
        else:
            result(mid, tree(revision=params.get("ref") or "main"))
    elif method == "repo/blob":
        result(mid, {"bytes_b64": base64.b64encode(b"needle is here\n").decode()})
    elif method == "search/code":
        query = params.get("q", "")
        if query.startswith("empty"):
            result(mid, {"items": [], "truncated": False})
        elif scenario == "partial":
            send({"jsonrpc": "2.0", "method": "$/partial", "params": {"id": mid, "items": [
                {"repo": "personal/project", "path": "needle.rs", "sha": "blob", "matches": ["needle"], "line": 1}
            ]}})
            fail(mid)
        elif scenario == "rate":
            fail(mid, "rate_limited", "RETRY_WINDOW", 37)
        elif scenario == "unknown":
            fail(mid, "future_kind", "FUTURE_FAILURE")
        elif scenario == "provider":
            fail(mid, "provider", "UPSTREAM_FAILURE")
        elif scenario == "long":
            fail(mid, message="SEARCH_TOOL_REQUIRED " + "detail " * 150 + "ERROR_END_MARKER")
        else:
            fail(mid)
    else:
        result(mid, {})
'''


def configured(tmp: Path, scenario="auth"):
    provider = tmp / "provider.py"
    provider.write_text(PROVIDER)
    log, marker = tmp / "calls.jsonl", tmp / "pending"
    config = tmp / "provider.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{provider}", "{log}", "{scenario}", "{marker}"]\n'
    )
    return config, log, marker


def calls(log):
    return [json.loads(line) for line in log.read_text().splitlines()]


def wait_for(predicate, description):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.01)
    raise AssertionError(description)


def test_fresh_and_warm_profiles_do_not_probe_inferred_organizations(tmp_path, binary):
    config, log, _ = configured(tmp_path)
    home = tmp_path / "home"
    fresh = states(run_headless(binary, "state\n", "--config", str(config), home=home))[0]
    assert fresh["popup"] is True
    assert fresh["browser"]["tree"]["phase"] == "idle"
    assert fresh["browser"]["pane"]["entry_count"] == 0

    state_path = home / "state" / "rootle" / "state.json"
    state_path.parent.mkdir(parents=True, exist_ok=True)
    legacy = {"version": 1, "recent_orgs": ["personal"], "recent_repos": ["personal/project"], "last_repo": "personal/project"}
    state_path.write_text(json.dumps(legacy))
    warm = states(run_headless(binary, "state\n", "--config", str(config), home=home))[0]
    assert warm["popup"] is False
    assert warm["search_view"] is False
    assert warm["browser"]["owner_kind"] == "unknown"
    assert state_path.read_text() == json.dumps(legacy)

    ready = states(run_headless(binary, "settle\nstate\n", "--config", str(config), "personal/project@topic", home=home))[0]
    tree = ready["browser"]["tree"]
    assert tree["phase"] == "ready"
    assert tree["request"]["repository"] == "personal/project"
    assert tree["request"]["revision"] == "topic"
    assert tree["entry_count"] == 3
    assert ready["browser"]["pane"]["entry_count"] == 2
    assert ready["browser"]["owner_kind"] == "unknown"
    assert not any(call["method"] == "org/repos" for call in calls(log))
    assert [call["params"].get("ref") for call in calls(log) if call["method"] == "repo/tree"] == ["topic"]
    saved = json.loads(state_path.read_text())
    assert saved["recent_orgs"] == legacy["recent_orgs"]
    assert saved["recent_repos"] == legacy["recent_repos"]


@pytest.mark.parametrize("scenario,kind,marker", [
    ("auth", "auth", "SEARCH_TOOL_REQUIRED"),
    ("provider", "provider", "UPSTREAM_FAILURE"),
    ("unknown", "other", "FUTURE_FAILURE"),
    ("rate", "rate_limited", "RETRY_WINDOW"),
])
def test_search_failure_is_typed_and_visible_not_empty(tmp_path, binary, scenario, kind, marker):
    config, _, _ = configured(tmp_path, scenario)
    out = run_headless(binary,
        "keys <esc><esc><space>gneedle<cr>\nsettle\nframe\nstate\n",
        "--config", str(config), home=tmp_path / "home", cols=140, rows=45)
    search = states(out)[0]["search"]
    assert search["phase"] == "failed"
    assert search["error"]["kind"] == kind
    assert marker in search["error"]["message"]
    assert "\x1b" not in search["error"]["message"]
    assert marker in frames(out)[0]
    assert search["error"]["retry_after_s"] == (37 if scenario == "rate" else None)
    assert search["request"]["query"] == "needle"


def test_unsupported_search_does_not_call_provider_search(tmp_path, binary):
    config, log, _ = configured(tmp_path, "unsupported")
    out = run_headless(binary,
        "keys <esc><esc><space>gneedle<cr>\nsettle\nstate\nframe\n",
        "--config", str(config), home=tmp_path / "home")
    state = states(out)[0]
    assert state["capabilities"]["code_search"] is False
    assert state["search"]["phase"] == "failed"
    assert not any(call["method"] == "search/code" for call in calls(log))
    assert state["search"]["error"]["message"] in frames(out)[0]


def test_partial_failure_survives_expansion_and_explicit_retry_clears_it(tmp_path, binary):
    config, _, _ = configured(tmp_path, "partial")
    out = run_headless(binary,
        "keys <esc><esc><space>gneedle<cr>\nsettle\nframe\nstate\n"
        "keys <cr>\nsettle\nframe\n"
        "keys <tab><bs><bs><bs><bs><bs><bs>empty<cr>\nsettle\nframe\nstate\n",
        "--config", str(config), home=tmp_path / "home", cols=140, rows=45)
    failed, retried = states(out)
    assert failed["search"]["phase"] == "failed"
    assert failed["search"]["retained_hit_count"] == 1
    assert failed["search"]["truncated"] is None
    for frame in frames(out)[:2]:
        assert "SEARCH_TOOL_REQUIRED" in frame
        assert "needle.rs" in frame
    assert retried["search"]["phase"] == "ready"
    assert retried["search"]["retained_hit_count"] == 0
    assert retried["search"]["error"] is None
    assert retried["search"]["request"]["query"] == "empty"
    assert "SEARCH_TOOL_REQUIRED" not in frames(out)[2]


@pytest.mark.parametrize("cols,rows", [(140, 45), (40, 10)])
def test_error_notice_is_scrollable_on_actual_terminal(tmp_path, binary, cols, rows):
    config, _, _ = configured(tmp_path, "long")
    tui = Tui(binary, cols=cols, rows=rows, args=["--config", str(config), "personal/project"]).start()
    try:
        tui.expect("README.md")
        tui.send(" g")
        tui.type_query("needle")
        tui.key("ENTER")
        tui.expect("SEARCH_TOOL_REQUIRED")
        tui.send("G")
        tui.send("kk")  # back above the trailing retry guidance at narrow heights
        wait_for(
            lambda: "ERROR_END_MARKER" in tui.screen().translate(str.maketrans("", "", " \n│┃")),
            "the wrapped provider message must remain reachable",
        )
        tui.key("ESC")
        tui.expect("README.md")
        assert "ERROR_END_MARKER" not in tui.screen()
        tui.send("q")
        assert tui.wait_exit() == 0
    finally:
        tui.stop()


def test_real_worker_reordering_cannot_replace_latest_tree(tmp_path, binary):
    config, _, marker = configured(tmp_path, "reorder")
    trace = tmp_path / "session.jsonl"
    tui = Tui(binary, cols=140, rows=45, args=["--log-file", str(trace), "--config", str(config), "personal/project"]).start()
    try:
        tui.expect("README.md")
        tui.send(" r")
        wait_for(lambda: marker.exists() and marker.read_text() == "2", "first reload did not reach provider")
        tui.send(" r")
        wait_for(lambda: marker.read_text() == "3", "second reload did not reach provider")
        tui.send(" r")
        tui.expect("LIVE_NEW.rs")
        wait_for(lambda: trace.exists() and trace.read_text().count('"tree_request_mismatch"') >= 2,
                 "both obsolete worker outcomes must reach the UI guard")
        screen = tui.screen()
        tui.send("q")
        assert tui.wait_exit() == 0
        assert "LIVE_NEW.rs" in screen
        assert "OBSOLETE" not in screen
    finally:
        tui.stop()
    trace_records(trace)


def test_outcome_metadata_redacts_submitted_query_and_failure_message(tmp_path, binary):
    config, _, _ = configured(tmp_path)
    for content in (False, True):
        name = "full" if content else "metadata"
        trace = tmp_path / f"{name}.jsonl"
        args = ["--config", str(config), "--log-file", str(trace)]
        if content:
            args.append("--log-content")
        out = run_headless(binary,
            "keys <esc><esc><space>gPRIVATE_QUERY_SENTINEL<cr>\nsettle\nframe\nstate\n",
            *args, home=tmp_path / name)
        assert states(out)[0]["search"]["phase"] == "failed"
        trace_records(trace)
        recorded = trace.read_text()
        assert ("PRIVATE_QUERY_SENTINEL" in recorded) is content
        assert ("SEARCH_TOOL_REQUIRED" in recorded) is content
