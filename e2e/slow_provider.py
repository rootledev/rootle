#!/usr/bin/env python3
"""Deterministic stdio provider for headless settle tests (e2e only).

NDJSON-RPC like crates/stdio/examples/fs_provider.py, but every
method's behavior is scripted on the command line:

    python3 slow_provider.py method=delay:<ms> method=error method=hang

delay sleeps before replying (past any quiet window), error replies
with a scripted failure, hang never replies (settle deadline). Every
unscripted method answers instantly from one hardcoded repo. The
tree→blob chain is real: opening local/alpha selects main.rs and
loads its preview.
"""

from __future__ import annotations

import base64
import json
import sys
import time

TREE = [
    {"path": "main.rs", "type": "blob", "sha": "a" * 64, "size": 48},
]

BLOB = b'fn main() {\n    println!("hello slow chain");\n}\n'


def handle(method: str, params: dict) -> dict:
    if method == "initialize":
        # orgs only — no log capability, so the preview band never
        # spawns its ambient last-commit fetch on top of the chain.
        return {
            "protocol": 1,
            "name": "slow",
            "icon": "folder",
            "capabilities": {"orgs": True},
        }
    if method == "search/repos":
        return {"items": [{"full_name": "local/alpha"}]}
    if method == "org/repos":
        return {"repos": ["alpha"]}
    if method == "repo/tree":
        return {"entries": TREE, "truncated": False, "branch": "main"}
    if method == "repo/blob":
        return {"bytes_b64": base64.b64encode(BLOB).decode()}
    raise ValueError(f"unknown method {method!r}")


def main() -> None:
    spec = dict(arg.split("=", 1) for arg in sys.argv[1:])
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(req.get("id"), int):
            continue  # notification — never reply
        try:
            mode = spec.get(req.get("method", ""))
            if mode == "hang":
                time.sleep(3600)
            if mode and mode.startswith("delay:"):
                time.sleep(int(mode.split(":", 1)[1]) / 1000)
            if mode == "error":
                raise ValueError("scripted failure")
            result = handle(req.get("method", ""), req.get("params") or {})
            reply = {"jsonrpc": "2.0", "id": req["id"], "result": result}
        except Exception as e:  # noqa: BLE001 — surfaced to the TUI
            reply = {"jsonrpc": "2.0", "id": req.get("id"),
                     "error": {"code": 1, "message": str(e)}}
        sys.stdout.write(json.dumps(reply) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
