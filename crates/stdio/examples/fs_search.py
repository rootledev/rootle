"""Query grammar and bounded filesystem search."""

from __future__ import annotations

import os
import shlex
from fs_common import ORG, SKIP_DIRS, repo_dir
from fs_storage import walk_tree


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
