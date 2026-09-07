"""Branch, file-history and blame operations over worktrees."""

from __future__ import annotations

import base64
import os
import subprocess
from fs_common import ORG, git, git_bytes, is_git_repo, repo_dir, sha256
from fs_storage import list_repos


def any_worktree(root: str) -> bool:
    """Capabilities are handshake-global: declare refs/log/blame when at
    least one served repo is a git worktree; non-worktree repos answer
    the revision methods with errors."""
    return any(is_git_repo(repo_dir(root, f"{ORG}/{r}")) for r in list_repos(root))


def git_refs(root: str, repo: str) -> dict:
    base = repo_dir(root, repo)
    if not is_git_repo(base):
        raise ValueError(f"{repo} is not a git worktree")
    default = git(base, "symbolic-ref", "--short", "HEAD").strip()
    branches = []
    for line in git(base, "branch", "--format=%(refname:short)%00%(objectname)").splitlines():
        if not line:
            continue
        name, _, sha = line.partition("\x00")
        branches.append(
            {"name": name, "sha": sha, **({"default": True} if name == default else {})}
        )
    tags = []
    for line in git(base, "tag", "--format=%(refname:short)%00%(objectname)").splitlines():
        if line:
            name, _, sha = line.partition("\x00")
            tags.append({"name": name, "sha": sha})
    return {"branches": branches, "tags": tags}


def walk_tree_at(root: str, repo: str, ref: str) -> list[dict]:
    """Tree at a ref: git ls-tree for the shape; content ids stay
    sha256-of-bytes (the fs scheme) so blobs round-trip through
    repo/blob regardless of which ref rootle is browsing."""
    base = repo_dir(root, repo)
    if not is_git_repo(base):
        raise ValueError(f"{repo} is not a git worktree")
    try:
        # -t: directories too — without it the switched tree lists
        # only blobs and the miller columns can't drill (the demo
        # caught this on camera).
        out = git(base, "ls-tree", "-r", "-t", ref)
    except subprocess.CalledProcessError:
        raise ValueError(f"unknown ref {ref!r}") from None
    entries = []
    for line in out.splitlines():
        meta, _, path = line.partition("\t")
        _mode, kind, _gitsha = meta.split()
        if kind == "blob":
            data = git_bytes(base, "show", f"{ref}:{path}")
            entries.append(
                {"path": path, "type": "blob", "sha": sha256(data), "size": len(data)}
            )
        elif kind == "tree":
            entries.append({"path": path, "type": "tree", "sha": sha256(path.encode())})
    return entries


def git_log(root: str, repo: str, path: str | None, ref: str | None, limit: int | None) -> dict:
    base = repo_dir(root, repo)
    if not is_git_repo(base):
        raise ValueError(f"{repo} is not a git worktree")
    want = min(limit or 50, 99)
    args = ["log", f"-n{want + 1}", "--format=%H%x00%s%x00%an%x00%aI"]
    if ref:
        args.append(ref)
    if path:
        args += ["--", path]
    try:
        out = git(base, *args)
    except subprocess.CalledProcessError as e:
        raise ValueError(e.stderr.strip() or "git log failed") from None
    items = []
    for line in out.splitlines():
        sha, _, rest = line.partition("\x00")
        subject, _, rest = rest.partition("\x00")
        author, _, date = rest.partition("\x00")
        items.append({"sha": sha, "subject": subject, "author": author, "date": date})
    truncated = len(items) > want
    return {"items": items[:want], "truncated": truncated}


def git_blob_at(root: str, repo: str, path: str, ref: str | None) -> dict:
    base = repo_dir(root, repo)
    if ref and is_git_repo(base):
        try:
            data = git_bytes(base, "show", f"{ref}:{path}")
        except subprocess.CalledProcessError:
            raise ValueError(f"no {path} at {ref!r}") from None
    else:
        full = os.path.join(base, path)
        if not os.path.isfile(full):
            raise ValueError(f"no {path} in {repo}")
        with open(full, "rb") as f:
            data = f.read()
    return {"bytes_b64": base64.b64encode(data).decode(), "sha": sha256(data)}


def git_blame(root: str, repo: str, path: str, ref: str | None) -> dict:
    base = repo_dir(root, repo)
    if not is_git_repo(base):
        raise ValueError(f"{repo} is not a git worktree")
    args = ["blame", "--line-porcelain"]
    if ref:
        args.append(ref)
    args += ["--", path]
    try:
        out = git(base, *args)
    except subprocess.CalledProcessError as e:
        raise ValueError(e.stderr.strip() or "git blame failed") from None
    # line-porcelain: each entry opens with "sha <orig> <final>
    # [<count>]", carries author fields (in full at least once per
    # sha), and closes with a tab-prefixed content line. Abbreviated
    # repeats reference the sha alone — author/date ride the first
    # occurrence.
    import datetime
    import re

    header = re.compile(r"^([0-9a-f]{40}) (\d+) (\d+)( \d+)?$")
    by_sha: dict[str, tuple[str, int]] = {}
    ranges = []
    sha = author = None
    ts = 0
    final = 0
    for line in out.splitlines():
        m = header.match(line)
        if m:
            sha = m.group(1)
            final = int(m.group(3))
            if sha in by_sha:
                author, ts = by_sha[sha]
            continue
        if line.startswith("author "):
            author = line[7:]
        elif line.startswith("author-time "):
            ts = int(line[12:])
        elif line.startswith("\t"):
            if sha is not None:
                by_sha.setdefault(sha, (author or "", ts))
                # Full ISO-8601 — the same shape repo/log's %aI emits,
                # so the suite can cross-check blame date == commit date.
                date = (
                    datetime.datetime.fromtimestamp(ts, datetime.timezone.utc).isoformat()
                    if ts
                    else ""
                )
                if ranges and ranges[-1]["sha"] == sha and ranges[-1]["end_line"] == final - 1:
                    ranges[-1]["end_line"] = final  # coalesce adjacent runs
                else:
                    ranges.append(
                        {
                            "start_line": final,
                            "end_line": final,
                            "sha": sha,
                            "author": author or "",
                            "date": date,
                        }
                    )
    return {"ranges": ranges}
