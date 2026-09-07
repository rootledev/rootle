"""Filesystem repository discovery and content lookup."""

from __future__ import annotations

import os
from fs_common import SKIP_DIRS, git, git_bytes, is_git_repo, repo_dir, sha256


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
    # v1.5: the sha may name a blob only visible at another ref —
    # scan git's trees (paths can differ in content across refs, so
    # dedupe by (ref, path), not path).
    base = repo_dir(root, repo)
    if is_git_repo(base):
        seen = set()
        for ref in git(base, "for-each-ref", "--format=%(refname)").splitlines():
            for line in git(base, "ls-tree", "-r", ref).splitlines():
                meta, _, path = line.partition("\t")
                if meta.split()[1] != "blob" or (ref, path) in seen:
                    continue
                seen.add((ref, path))
                data = git_bytes(base, "show", f"{ref}:{path}")
                if sha256(data) == sha:
                    return data
    raise ValueError(f"no blob {sha} in {repo}")
