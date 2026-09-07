"""Session tracing through the real binary: privacy, causality and shutdown."""
from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
from pathlib import Path

from conftest import make_git_root
from headless import fs_config, frames, run_headless
from test_commit_viewer import OPEN_PROJ
from tui import hermetic_env


def trace_records(path: Path) -> list[dict]:
    records = [json.loads(line) for line in path.read_text().splitlines()]
    assert records[0]["event"] == "session_start"
    assert records[-1]["event"] == "trace_end"
    assert records[-1]["fields"]["complete"] is True
    assert [record["seq"] for record in records] == list(range(1, len(records) + 1))
    clocks = [record["elapsed_us"] for record in records]
    assert clocks == sorted(clocks)
    return records


def decoded_frames(records: list[dict]) -> list[str]:
    return [
        "\n".join("".join(run[1] * run[0] for run in row) for row in record["fields"]["cells"])
        for record in records if record["event"] == "render" and "cells" in record["fields"]
    ]


def test_content_policy_and_correlated_commit_capture(binary, tmp_path):
    root = make_git_root(tmp_path)
    repository = root / "proj"
    (repository / "main.rs").write_text('fn main() {\n    println!("SOURCE_TRACE_SENTINEL");\n}\n')
    git_env = hermetic_env(tmp_path / "git-home", {
        "GIT_AUTHOR_NAME": "Trace Fixture", "GIT_AUTHOR_EMAIL": "trace@example.test",
        "GIT_COMMITTER_NAME": "Trace Fixture", "GIT_COMMITTER_EMAIL": "trace@example.test",
        "GIT_AUTHOR_DATE": "2026-09-07T10:00:00Z", "GIT_COMMITTER_DATE": "2026-09-07T10:00:00Z",
        "GIT_CONFIG_NOSYSTEM": "1",
    })
    subprocess.run(["git", "-C", str(repository), "commit", "-qam", "trace fixture"],
                   env=git_env, check=True, capture_output=True)
    config = fs_config(tmp_path, root=root)
    script = (OPEN_PROJ + "keys l\nsettle\nkeys <space>ph\nsettle\nkeys d\nsettle\n"
              "frame\nframe\nkeys <cr>\nframe\nkeys <esc><esc><esc><esc>\n"
              "keys <space>s\nkeys INPUT_TRACE_SENTINEL\nframe\n")
    for content in (False, True):
        name = "full" if content else "metadata"
        path = tmp_path / f"{name}.jsonl"
        args = ["--config", str(config), "--log-file", str(path)]
        if content:
            args.append("--log-content")
        output = run_headless(binary, script, *args, home=tmp_path / name, cols=110)
        assert "SOURCE_TRACE_SENTINEL" in frames(output)[2]
        records = trace_records(path)
        render = [record["fields"] for record in records if record["event"] == "render"]
        assert render[1]["frame_number"] == render[0]["frame_number"] + 1
        assert render[0]["cell_hash"] == render[1]["cell_hash"]
        job = next(record for record in records
                   if record["event"] == "job_started" and record["fields"].get("job") == "commit")
        request = next(record for record in records
                       if record["event"] == "rpc_message" and record["fields"].get("method") == "repo/commit")
        assert request["operation_id"] == job["operation_id"]
        assert any(record["event"] == "rpc_message"
                   and record["fields"].get("dir") == "rx"
                   and record["fields"].get("id") == request["fields"]["id"]
                   and record["fields"].get("session") == request["fields"]["session"]
                   for record in records)
        rpc_text = json.dumps([record for record in records if record["event"] == "rpc_message"])
        assert "SOURCE_TRACE_SENTINEL" not in rpc_text
        if content:
            assert any("SOURCE_TRACE_SENTINEL" in frame for frame in decoded_frames(records))
            assert "INPUT_TRACE_SENTINEL" in path.read_text()
        else:
            assert all("cells" not in record for record in render)
            assert "SOURCE_TRACE_SENTINEL" not in path.read_text()
            assert "INPUT_TRACE_SENTINEL" not in path.read_text()
        assert path.stat().st_mode & 0o077 == 0


def test_trace_selection_refusal_and_cli_errors_finalize(binary, tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    environment_path = tmp_path / "environment.jsonl"
    chosen = tmp_path / "chosen.jsonl"
    env = hermetic_env(home, {"ROOTLE_TRACE": str(environment_path)})
    listed = subprocess.run([str(binary), "provider", "list", "--json", "--log-file", str(chosen)],
                            env=env, capture_output=True, text=True, check=True)
    assert json.loads(listed.stdout) == []
    assert not environment_path.exists(), "explicit CLI selection overrides the environment"
    trace_records(chosen)
    original = chosen.read_bytes()
    refused = subprocess.run([str(binary), "--headless", "-", "--log-file", str(chosen)],
                             input="frame\n", env=env, capture_output=True, text=True)
    assert refused.returncode != 0 and refused.stdout == ""
    assert chosen.read_bytes() == original
    failed_path = tmp_path / "failed-command.jsonl"
    failed = subprocess.run([str(binary), "provider", "pin", "not-installed", "--log-file", str(failed_path)],
                            env=env, capture_output=True, text=True)
    assert failed.returncode != 0
    records = trace_records(failed_path)
    assert records[-2]["event"] == "session_end"
    assert records[-2]["fields"]["outcome"] == "error"
    auto_env = hermetic_env(tmp_path / "automatic")
    auto = subprocess.run([str(binary), "--log", "--headless", "-"], input="frame\n",
                          env=auto_env, capture_output=True, text=True, check=True)
    paths = list((tmp_path / "automatic" / "state" / "rootle" / "logs").glob("*.jsonl"))
    assert len(paths) == 1, auto.stderr
    trace_records(paths[0])
    assert paths[0].stat().st_mode & 0o077 == 0
    content_without_session = subprocess.run([str(binary), "--headless", "-", "--log-content"],
                                             input=b"frame\n", env=hermetic_env(home), capture_output=True)
    assert content_without_session.returncode != 0


FAULT_PROVIDER = r'''
import json, sys, time
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    identifier = request["id"]
    if request["method"] == "initialize":
        result = {"protocol": 1, "name": "fixture", "capabilities": {"orgs": False}}
        print(json.dumps({"jsonrpc": "2.0", "id": identifier, "result": result}), flush=True)
    elif request["method"] == "search/repos":
        time.sleep(0.2)
        print("not-json RPC_TRACE_SECRET", flush=True)
        print(json.dumps({"jsonrpc": "2.0", "id": 99999, "result": "RPC_TRACE_SECRET"}), flush=True)
        print("STDERR_TRACE_MARKER", file=sys.stderr, flush=True)
        print(json.dumps({"jsonrpc": "2.0", "id": identifier, "error": {
            "code": -32000, "message": "RPC_TRACE_SECRET", "data": {"kind": "network"}}}), flush=True)
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": identifier, "result": {"repos": []}}), flush=True)
'''


def test_late_provider_failures_and_sensitive_transport_surfaces(binary, tmp_path):
    provider = tmp_path / "provider.py"
    provider.write_text(FAULT_PROVIDER)
    config = tmp_path / "config.toml"
    config.write_text('[provider]\nkind = "stdio"\ncommand = ' +
                      json.dumps([sys.executable, str(provider), "ARGV_TRACE_SECRET"]) + "\n")
    for content in (False, True):
        name = "full" if content else "metadata"
        path = tmp_path / f"{name}.jsonl"
        args = ["--config", str(config), "--log-file", str(path)]
        if content:
            args.append("--log-content")
        run_headless(binary, "keys fixture<cr>\nkeys <esc>\nsettle\nframe\n", *args,
                     home=tmp_path / name, env_extra={"ROOTLE_TOKEN": "ENV_TRACE_SECRET"})
        records = trace_records(path)
        assert any(record["event"] == "job_rejected"
                   and record["fields"].get("reason") in {"search_popup_closed", "stale_search_generation"}
                   for record in records)
        assert any(record["event"] == "rpc_message" and record["fields"].get("routed") == "unknown_id"
                   for record in records)
        saved = path.read_text()
        assert "ARGV_TRACE_SECRET" not in saved
        assert "ENV_TRACE_SECRET" not in saved
        assert "RPC_TRACE_SECRET" not in saved
        assert ("STDERR_TRACE_MARKER" in saved) == content


def test_full_stderr_capture_cannot_wait_for_an_inherited_pipe(binary, tmp_path):
    release = tmp_path / "release-descendant"
    provider = tmp_path / "inherited.py"
    provider.write_text('''import json, subprocess, sys
child = None
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request: continue
    if request["method"] == "initialize":
        if child is None:
            child = subprocess.Popen([sys.executable, "-c",
                "import pathlib,sys,time; p=pathlib.Path(sys.argv[1]);\\nwhile not p.exists(): time.sleep(.01)", sys.argv[1]],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
        result = {"protocol": 1, "name": "fixture", "capabilities": {"orgs": False}}
    else: result = {"repos": []}
    print(json.dumps({"jsonrpc":"2.0", "id":request["id"], "result":result}), flush=True)
''')
    config = tmp_path / "config.toml"
    config.write_text('[provider]\nkind = "stdio"\ncommand = ' +
                      json.dumps([sys.executable, str(provider), str(release)]) + "\n")
    path = tmp_path / "inherited.jsonl"
    process = subprocess.Popen([str(binary), "--config", str(config), "--headless", "-",
                                "--log-file", str(path), "--log-content"],
                               env=hermetic_env(tmp_path / "home"), stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    try:
        stdout, stderr = process.communicate(b"keys <esc><esc>q\n", timeout=5)
        assert process.returncode == 0, stderr.decode()
        trace_records(path)
    finally:
        release.touch()
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.communicate()
