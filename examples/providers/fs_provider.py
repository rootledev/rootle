#!/usr/bin/env python3
"""rootle stdio provider over a local directory (plans/0005).

Reference implementation of the rootle provider protocol: NDJSON-RPC 2.0
on stdin/stdout. Serves <root>/<repo> as repos under the "local" org —
useful as a template for wrapping internal systems, as an offline dev
backend, and as documentation-by-example of the protocol.

    python3 fs_provider.py ~/code            # serve ~/code/* as repos
    rootle --config provider.toml               # [provider] kind="stdio"

Protocol v1 methods:
    initialize            -> {protocol, name, capabilities}
    search/repos  {query} -> {items: [{full_name} | {org}]}
    org/repos     {org}   -> {repos: [name]}
    repo/tree     {repo}  -> {entries: [{path, type, sha, size?}], truncated, branch}
    repo/blob     {repo, sha} -> {bytes_b64}
    search/code   {q}     -> {items: [{repo, path, sha, branch, matches}]}

Contract: blob shas are content hashes (sha256) — they change when
content changes, which is what rootle's cache requires.
"""

import base64
import hashlib
import json
import os
import pathlib
import subprocess
import sys
from typing import Iterator

ORG = "local"
SKIP_DIRS = {".git", "__pycache__", "target", "node_modules"}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def list_repos(root: str) -> list[str]:
    """Repo names under ORG, nested paths included ("nested/sub"):
    a directory is a repo when it holds files directly; directories
    with only subdirectories keep descending (bounded). Multi-slash
    ids are legal — rootle treats repos as opaque strings."""
    def walk(dir: str, rel: str, depth: int) -> list[str]:
        out: list[str] = []
        for d in sorted(os.listdir(dir)):
            full = os.path.join(dir, d)
            if not os.path.isdir(full) or d in SKIP_DIRS:
                continue
            child_rel = f"{rel}/{d}" if rel else d
            entries = os.listdir(full)
            has_file = any(os.path.isfile(os.path.join(full, e)) for e in entries)
            is_worktree = ".git" in entries
            if has_file or is_worktree:
                # A directory with files — or a git worktree, whose root
                # may hold only subdirs — is a repo (a forge project
                # root); never descend into one.
                out.append(child_rel)
            elif depth < 3:
                out.extend(walk(full, child_rel, depth + 1))
        return out

    return walk(root, "", 0)


def repo_dir(root: str, repo: str) -> str:
    if "/" not in repo:
        raise ValueError(f"bad repo id {repo!r}")
    path = os.path.join(root, repo.split("/", 1)[1])
    if not os.path.isdir(path):
        raise ValueError(f"unknown repo {repo!r}")
    return path


def walk_tree(root: str, repo: str) -> list[dict]:
    """Recursive entries: blobs content-hashed, dirs path-hashed."""
    base = repo_dir(root, repo)
    entries = []
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for name in sorted(dirnames):
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, base)
            entries.append({"path": rel, "type": "tree", "sha": sha256(rel.encode())})
        for name in sorted(filenames):
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, base)
            with open(full, "rb") as f:
                data = f.read()
            entries.append(
                {"path": rel, "type": "blob", "sha": sha256(data), "size": len(data)}
            )
    return entries


def blob_by_sha(root: str, repo: str, sha: str) -> bytes:
    for entry in walk_tree(root, repo):
        if entry["type"] == "blob" and entry["sha"] == sha:
            with open(os.path.join(repo_dir(root, repo), entry["path"]), "rb") as f:
                return f.read()
    raise ValueError(f"no blob {sha} in {repo}")


def parse_query(q: str) -> tuple[str, str | None, str | None, str | None]:
    """Split a rootle code query into (terms, repo, org, extension)."""
    repo = org = ext = None
    terms = []
    for token in q.split():
        if token.startswith("repo:"):
            repo = token[5:]
        elif token.startswith("org:"):
            org = token[4:]
        elif token.startswith("extension:"):
            ext = token[10:]
        elif token.startswith("path:"):
            terms.append(token[5:])  # path match ≈ term match for fs
        else:
            terms.append(token)
    return " ".join(terms), repo, org, ext


def search_code(root: str, q: str) -> list[dict]:
    terms, repo_scope, _org, ext = parse_query(q)
    needles = [t.lower() for t in terms.split() if t]
    repos = [f"{ORG}/{repo_scope.split('/', 1)[1]}"] if repo_scope else [
        f"{ORG}/{d}" for d in sorted(os.listdir(root))
        if os.path.isdir(os.path.join(root, d)) and d not in SKIP_DIRS
    ]
    items = []
    for repo in repos:
        if not os.path.isdir(os.path.join(root, repo.split("/", 1)[1])):
            continue
        for entry in walk_tree(root, repo):
            if entry["type"] != "blob":
                continue
            if ext and not entry["path"].lower().endswith("." + ext.lstrip(".")):
                continue
            full = os.path.join(repo_dir(root, repo), entry["path"])
            try:
                text = open(full, encoding="utf-8", errors="replace").read()
            except OSError:
                continue
            if text.startswith("\x00") or "\x00" in text[:8192]:
                continue  # binary
            matched = [n for n in needles if n in text.lower()]
            if needles and not matched:
                continue
            items.append(
                {
                    "repo": repo,
                    "path": entry["path"],
                    "sha": entry["sha"],
                    "branch": "main",
                    "matches": matched,
                }
            )
    return items


def search_code_batches(root: str, q: str) -> Iterator[list[dict]]:
    """v1.3 progressive search: yield per-repo batches; the caller
    streams each as a $/partial notification."""
    terms, repo_scope, _org, ext = parse_query(q)
    needles = [t.lower() for t in terms.split() if t]
    repos = [f"{ORG}/{repo_scope.split('/', 1)[1]}"] if repo_scope else [
        f"{ORG}/{d}" for d in sorted(os.listdir(root))
        if os.path.isdir(os.path.join(root, d)) and d not in SKIP_DIRS
    ]
    for repo in repos:
        if not os.path.isdir(os.path.join(root, repo.split("/", 1)[1])):
            continue
        batch = []
        for entry in walk_tree(root, repo):
            if entry["type"] != "blob":
                continue
            if ext and not entry["path"].lower().endswith("." + ext.lstrip(".")):
                continue
            full = os.path.join(repo_dir(root, repo), entry["path"])
            try:
                text = open(full, encoding="utf-8", errors="replace").read()
            except OSError:
                continue
            if text.startswith("\x00") or "\x00" in text[:8192]:
                continue  # binary
            matched = [n for n in needles if n in text.lower()]
            if needles and not matched:
                continue
            batch.append(
                {
                    "repo": repo,
                    "path": entry["path"],
                    "sha": entry["sha"],
                    "branch": "main",
                    "matches": matched,
                }
            )
        if batch:
            yield batch


def handle(root: str, method: str, params: dict) -> dict:
    if method == "initialize":
        return {
            "protocol": 1,
            "name": "fs",
            # v1.3: the modeline icon — a builtin name rootle maps to
            # its Nerd Font glyph when nerd_font is on.
            "icon": "folder",
            "capabilities": {"orgs": True, "code_search": True},
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
        return {
            "entries": walk_tree(root, repo),
            "truncated": False,
            "branch": "main",
        }
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
            url += f"#L{line}"
        return {"url": url}
    if method == "org/url":
        return {"url": pathlib.Path(root).resolve().as_uri()}
    if method == "repo/blob":
        data = blob_by_sha(root, params["repo"], params["sha"])
        return {"bytes_b64": base64.b64encode(data).decode()}
    if method == "search/code":
        return {"items": search_code(root, params.get("q", ""))}
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
                for batch in search_code_batches(root, params.get("q", "")):
                    note = {
                        "jsonrpc": "2.0",
                        "method": "$/partial",
                        "params": {"id": partial, "items": batch},
                    }
                    sys.stdout.write(json.dumps(note) + "\n")
                    sys.stdout.flush()
                result = {"items": [], "truncated": False}
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
