"""Filesystem provider path, content-ID and Git primitives."""

from __future__ import annotations

import hashlib
import os
import subprocess


ORG = "local"


SKIP_DIRS = {".git", "__pycache__", "target", "node_modules"}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def repo_dir(root: str, repo: str) -> str:
    if "/" not in repo:
        raise ValueError(f"bad repo id {repo!r}")
    path = os.path.join(root, repo.split("/", 1)[1])
    if not os.path.isdir(path):
        raise ValueError(f"unknown repo {repo!r}")
    return path


def git(repo_abs: str, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", repo_abs, *args], capture_output=True, text=True, check=True
    ).stdout


def git_bytes(repo_abs: str, *args: str) -> bytes:
    return subprocess.run(
        ["git", "-C", repo_abs, *args], capture_output=True, check=True
    ).stdout


def is_git_repo(path: str) -> bool:
    return os.path.isdir(os.path.join(path, ".git")) or os.path.isfile(
        os.path.join(path, ".git")
    )
