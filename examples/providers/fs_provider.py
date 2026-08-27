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

Advisory params honored: `partial` (v1.3 — stream $/partial batches,
metadata-only reply) and `limit` (v1.4 — stop scanning at ~N hits,
set truncated: true).

Contract: blob shas are content hashes (sha256) — they change when
content changes, which is what rootle's cache requires.
"""

import base64
import hashlib
import json
import os
import pathlib
import shlex
import subprocess
import sys

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


LANG_EXTS = {
    "rust": ["rs"], "python": ["py", "pyi"], "javascript": ["js", "jsx", "mjs"],
    "typescript": ["ts", "tsx"], "go": ["go"], "c": ["c", "h"],
    "c++": ["cpp", "cc", "hpp"], "java": ["java"], "ruby": ["rb"],
    "shell": ["sh", "bash"], "bash": ["sh", "bash"], "toml": ["toml"],
    "yaml": ["yaml", "yml"], "json": ["json"], "markdown": ["md"],
    "html": ["html"], "css": ["css"],
}


def parse_query(q: str) -> dict:
    """Split a rootle code query (plans/0012 M1 grammar): quoted
    literals are one term; `-term` / `NOT term` negate;
    `language:`/`extension:` filter by extension. Scope qualifiers
    (`repo:`/`org:`) scope the walk; `path:` counts as a term (path
    match ≈ term match for fs)."""
    try:
        tokens = shlex.split(q)
    except ValueError:  # unterminated quote — the phrase is one term
        tokens = shlex.split(q + '"')
    parsed: dict = {
        "terms": [], "negated": [], "repo": None, "org": None,
        "ext": None, "lang": None, "neglang": None,
    }
    i = 0
    while i < len(tokens):
        tok = tokens[i]
        neg = False
        if tok == "NOT" and i + 1 < len(tokens):
            neg = True
            i += 1
            tok = tokens[i]
        elif tok.startswith("-"):
            neg = True
            tok = tok[1:]
        if not tok:
            pass
        elif tok.startswith("repo:"):
            parsed["repo"] = tok[5:]
        elif tok.startswith("org:"):
            parsed["org"] = tok[4:]
        elif tok.startswith("extension:"):
            parsed["ext"] = tok[10:].lstrip(".")
        elif tok.startswith("language:"):
            parsed["neglang" if neg else "lang"] = tok[9:].lower()
        elif tok.startswith("path:"):
            (parsed["negated"] if neg else parsed["terms"]).append(tok[5:])
        else:
            (parsed["negated"] if neg else parsed["terms"]).append(tok)
        i += 1
    return parsed


def file_in_scope(path: str, text: str, parsed: dict) -> list[str] | None:
    """The matched needles, or None when the file is out — negation
    and language: are post-filters (the fs backend has no query
    grammar of its own to translate to)."""
    low = text.lower()
    needles = [t.lower() for t in parsed["terms"]]
    matched = [n for n in needles if n in low]
    if needles and not matched:
        return None
    path_low = path.lower()
    for n in parsed["negated"]:
        if n.lower() in low or n.lower() in path_low:
            return None
    ext = path.rsplit(".", 1)[-1].lower() if "." in path else ""
    if parsed["lang"]:
        exts = LANG_EXTS.get(parsed["lang"], [parsed["lang"]])
        if ext not in exts:
            return None
    if parsed["neglang"]:
        exts = LANG_EXTS.get(parsed["neglang"], [parsed["neglang"]])
        if ext in exts:
            return None
    return matched


def search_code(root: str, q: str, limit: int | None) -> tuple[list[dict], bool]:
    """One-shot search. Honors the v1.4 advisory `limit`: stop
    scanning at ~N and set `truncated` — which means exactly what a
    provider's own cap means (doc/provider-protocol.md)."""
    parsed = parse_query(q)
    repo_scope = parsed["repo"]
    ext = parsed["ext"]
    repos = [f"{ORG}/{repo_scope.split('/', 1)[1]}"] if repo_scope else [
        f"{ORG}/{d}" for d in sorted(os.listdir(root))
        if os.path.isdir(os.path.join(root, d)) and d not in SKIP_DIRS
    ]
    items = []
    truncated = False
    for repo in repos:
        if truncated:
            break
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
            matched = file_in_scope(entry["path"], text, parsed)
            if matched is None:
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
            if limit is not None and len(items) >= limit:
                truncated = True
                break
    return items, truncated


def search_code_batches(
    root: str, q: str, limit: int | None
) -> tuple[list[list[dict]], bool]:
    """v1.3 progressive search: per-repo batches, each streamed by the
    caller as a $/partial notification. Honors the v1.4 advisory
    `limit`: stop scanning at ~N (batch granularity) and report
    `truncated` in the metadata-only reply."""
    parsed = parse_query(q)
    repo_scope = parsed["repo"]
    ext = parsed["ext"]
    repos = [f"{ORG}/{repo_scope.split('/', 1)[1]}"] if repo_scope else [
        f"{ORG}/{d}" for d in sorted(os.listdir(root))
        if os.path.isdir(os.path.join(root, d)) and d not in SKIP_DIRS
    ]
    batches: list[list[dict]] = []
    sent = 0
    truncated = False
    for repo in repos:
        if truncated:
            break
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
            matched = file_in_scope(entry["path"], text, parsed)
            if matched is None:
                continue
            # v1.3: we know the real line — first one matching the first
            # needle (the backend hands us offsets nobody has).
            line = 1
            if matched:
                lowered = text.lower()
                for n, ln in enumerate(lowered.splitlines(), start=1):
                    if matched[0] in ln:
                        line = n
                        break
            batch.append(
                {
                    "repo": repo,
                    "path": entry["path"],
                    "sha": entry["sha"],
                    "branch": "main",
                    "matches": matched,
                    "line": line,
                }
            )
            if limit is not None and sent + len(batch) >= limit:
                truncated = True
                break
        if batch:
            batches.append(batch)
            sent += len(batch)
    return batches, truncated


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
