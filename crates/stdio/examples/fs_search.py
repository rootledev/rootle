"""Query grammar and bounded filesystem search; copy languages.tsv beside this module."""

from __future__ import annotations

import os
from pathlib import Path
from fs_common import ORG, SKIP_DIRS, repo_dir
from fs_storage import walk_tree


# The reference fixture reads the same catalog embedded by rootle. No second
# alias/extension table can drift from local search or syntax highlighting.
_CATALOG = Path(__file__).with_name("languages.tsv")
LANG_EXTS = {}
for _row in _CATALOG.read_text().splitlines():
    _name, _aliases, _extensions = _row.split("\t")
    for _alias in [_name, *filter(None, _aliases.split(","))]:
        LANG_EXTS[_alias] = _extensions.split(",")


def tokens(q: str):
    """Strop-style quotes; escapes only quote/backslash inside quotes."""
    at = 0
    while at < len(q):
        if q[at].isspace():
            at += 1
            continue
        quoted = q[at] in "'\""
        text = []
        quote = None
        while at < len(q):
            c = q[at]
            if quote is None and c.isspace():
                break
            at += 1
            if quote is not None:
                if c == quote:
                    quote = None
                elif c == "\\" and at < len(q) and q[at] in (quote, "\\"):
                    text.append(q[at])
                    at += 1
                else:
                    text.append(c)
            elif c in "'\"":
                quote = c
            else:
                text.append(c)
        if quote is not None:
            raise ValueError("unclosed quote")
        yield "".join(text), quoted


def parse_query(q: str) -> dict:
    """Raw wire grammar: literal terms ANDed, repeated filters ORed,
    exclusions win. Unknown/incomplete qualifiers fail instead of broadening."""
    parsed = {key: [] for key in (
        "terms", "negated", "language", "-language", "extension", "-extension",
        "path", "-path", "repo", "-repo", "org", "-org",
    )}
    negate_next = False
    pending_not = False
    for tok, quoted in tokens(q):
        if not quoted and tok == "NOT":
            negate_next = not negate_next
            pending_not = True
            continue
        prefix_negated = not quoted and tok.startswith("-")
        negated = negate_next ^ prefix_negated
        negate_next = False
        pending_not = False
        if prefix_negated:
            tok = tok[1:]
            if not tok:
                raise ValueError("negation expects a term or qualifier")
        if not quoted and tok in ("AND", "OR"):
            raise ValueError(f"unsupported Boolean operator: {tok}")
        key, colon, value = tok.partition(":")
        known = key in (
            "language", "extension", "path", "repo", "org", "fpath", "glob",
            "hidden", "ignored", "case", "text", "regex", "kind", "type",
        )
        qualifier = (not quoted and colon and (
            known or (len(key) >= 3 and key.isascii() and key.isalpha()
                      and not value.startswith(("//", ":")))
        ))
        if qualifier:
            if not value:
                raise ValueError(f"{key}: expects a value")
            if key not in ("language", "extension", "path", "repo", "org"):
                raise ValueError(f"unsupported qualifier: {key}:")
            if key == "language" and value not in LANG_EXTS:
                raise ValueError(f"unknown language: {value}")
            if key == "extension":
                value = value.lstrip(".").lower()
                if not value:
                    raise ValueError("extension: expects a value")
            parsed[("-" if negated else "") + key].append(value)
        elif tok:
            parsed["negated" if negated else "terms"].append(tok)
    if pending_not:
        raise ValueError("NOT expects a term or qualifier")
    return parsed


def alternatives(parsed: dict, key: str, matches) -> bool:
    return ((not parsed[key] or any(matches(v) for v in parsed[key]))
            and not any(matches(v) for v in parsed["-" + key]))


def scoped_repos(root: str, parsed: dict) -> list[str]:
    return [
        f"{ORG}/{name}" for name in sorted(os.listdir(root))
        if name not in SKIP_DIRS and os.path.isdir(os.path.join(root, name))
        and alternatives(parsed, "repo", lambda value: value == f"{ORG}/{name}")
        and alternatives(parsed, "org", lambda value: value == ORG)
    ]


def file_in_scope(path: str, text: str, parsed: dict) -> list[str] | None:
    low = text.lower()
    needles = [t.lower() for t in parsed["terms"]]
    if not all(n in low for n in needles):
        return None
    path_low = path.lower()
    if any(n.lower() in low or n.lower() in path_low for n in parsed["negated"]):
        return None
    base = path.replace("\\", "/").rsplit("/", 1)[-1]
    _, dot, ext = base.rpartition(".")
    ext = ext if dot else ""
    if not alternatives(parsed, "language", lambda value: ext in LANG_EXTS[value]):
        return None
    if not alternatives(parsed, "extension", lambda value: ext.lower() == value):
        return None
    if not alternatives(parsed, "path", lambda value: (
        value in path if any(c.isupper() for c in value) else value in path_low
    )):
        return None
    return needles


def search_code(root: str, q: str, limit: int | None) -> tuple[list[dict], bool]:
    """One-shot search. Honors the v1.4 advisory `limit`: stop
    scanning at ~N and set `truncated` — which means exactly what a
    provider's own cap means (doc/provider-protocol.md)."""
    parsed = parse_query(q)
    repos = scoped_repos(root, parsed)
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
    repos = scoped_repos(root, parsed)
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
