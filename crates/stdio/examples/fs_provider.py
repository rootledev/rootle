#!/usr/bin/env python3
"""NDJSON-RPC reference adapter; invoke this entry point beside its fs_* support modules."""

from __future__ import annotations

import base64
import json
import os
import pathlib
import sys
from fs_commit import git_commit
from fs_common import ORG, repo_dir
from fs_revisions import any_worktree, git_blame, git_blob_at, git_log, git_refs, walk_tree_at
from fs_search import search_code, search_code_batches
from fs_storage import blob_by_sha, list_repos, walk_tree


def handle(root: str, method: str, params: dict) -> dict:
    if method == "initialize":
        caps = {"orgs": True, "code_search": True}
        if any_worktree(root):
            # v1.5: revision methods are git-backed when the served
            # repos are worktrees; v1.6 adds commit detail.
            caps |= {"refs": True, "log": True, "blame": True, "commit": True}
        return {
            "protocol": 1,
            "name": "fs",
            # v1.3: the modeline icon — a builtin name rootle maps to
            # its Nerd Font glyph when nerd_font is on.
            "icon": "folder",
            "capabilities": caps,
        }
    if method == "search/repos":
        query = params.get("query", "").lower()
        items = [
            {"full_name": f"{ORG}/{d}"}
            for d in list_repos(root)
            if query in d.lower()
        ]
        if not items:
            items.append({"org": ORG})
        return {"items": items[:20]}
    if method == "org/repos":
        return {"repos": list_repos(root)}
    if method == "repo/tree":
        repo = params["repo"]
        ref = params.get("ref")
        # v1.5: a ref walks git's tree; absent, the filesystem as-is.
        entries = walk_tree_at(root, repo, ref) if ref else walk_tree(root, repo)
        return {
            "entries": entries,
            "truncated": False,
            "branch": ref or "main",
        }
    if method == "repo/refs":
        return git_refs(root, params["repo"])
    if method == "repo/log":
        return git_log(
            root,
            params["repo"],
            params.get("path"),
            params.get("ref"),
            params.get("limit"),
        )
    if method == "repo/blob_at":
        return git_blob_at(root, params["repo"], params["path"], params.get("ref"))
    if method == "repo/blame":
        return git_blame(root, params["repo"], params["path"], params.get("ref"))
    if method == "repo/commit":
        return git_commit(root, params["repo"], params["sha"])
    if method == "repo/clone_url":
        # Cloning a local dir: the filesystem path IS the remote.
        return {"clone_url": repo_dir(root, params["repo"])}
    if method == "repo/web_url":
        base = pathlib.Path(repo_dir(root, params["repo"])).resolve().as_uri()
        path = params.get("path", "")
        line = params.get("line")
        is_file = params.get("is_file", False)
        url = f"{base}/{path}" if path else base
        if is_file and line:
            end = params.get("end_line")
            # v1.5: a selection yanks a range anchor.
            url += f"#L{line}-L{end}" if end and end > line else f"#L{line}"
        return {"url": url}
    if method == "org/url":
        return {"url": pathlib.Path(root).resolve().as_uri()}
    if method == "repo/blob":
        data = blob_by_sha(root, params["repo"], params["sha"])
        return {"bytes_b64": base64.b64encode(data).decode()}
    if method == "search/code":
        items, truncated = search_code(root, params.get("q", ""), params.get("limit"))
        return {"items": items, "truncated": truncated}
    raise ValueError(f"unknown method {method!r}")


def main() -> None:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    root = os.path.abspath(sys.argv[1])
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(req.get("id"), int):
            # Notification (e.g. $/cancelRequest) — never reply.
            continue
        partial = None
        try:
            params = req.get("params") or {}
            if req.get("method") == "search/code" and params.get("partial"):
                # v1.3: stream batches as $/partial notifications keyed
                # by the request id; the reply is metadata-only.
                partial = req.get("id")
                batches, truncated = search_code_batches(
                    root, params.get("q", ""), params.get("limit")
                )
                for batch in batches:
                    note = {
                        "jsonrpc": "2.0",
                        "method": "$/partial",
                        "params": {"id": partial, "items": batch},
                    }
                    sys.stdout.write(json.dumps(note) + "\n")
                    sys.stdout.flush()
                result = {"items": [], "truncated": truncated}
            else:
                result = handle(root, req.get("method", ""), params)
            reply = {"jsonrpc": "2.0", "id": req.get("id"), "result": result}
        except Exception as e:  # noqa: BLE001 — surfaced to the TUI
            reply = {
                "jsonrpc": "2.0",
                "id": req.get("id"),
                "error": {"code": 1, "message": str(e)},
            }
        sys.stdout.write(json.dumps(reply) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
