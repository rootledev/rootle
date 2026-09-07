"""Commit detail over git's NUL-delimited path records and first-parent diff."""

from __future__ import annotations

import subprocess
from fs_common import git_bytes, is_git_repo, repo_dir

COMMIT_FILE_LIMIT = 500


def git_commit(root: str, repo: str, sha: str) -> dict:
    base = repo_dir(root, repo)
    if not is_git_repo(base):
        raise ValueError(f"{repo} is not a git worktree")
    try:
        resolved = git_bytes(base, "rev-parse", "--verify", "--end-of-options", f"{sha}^{{commit}}").decode().strip()
        metadata = git_bytes(base, "show", "-s", "--format=%H%x00%an%x00%aI%x00%P%x00%B", resolved).decode("utf-8", "replace")
        commit_sha, author, date, parents_text, message = metadata.split("\x00", 4)
        parents = parents_text.split()
        revisions = [parents[0], resolved] if parents else [resolved]
        options = ["diff-tree", "-r", "-M", "--root", "--no-commit-id", "--no-ext-diff"]
        records = changed_paths(git_bytes(base, *options, "--name-status", "-z", *revisions))
        truncated = len(records) > COMMIT_FILE_LIMIT
        records = records[:COMMIT_FILE_LIMIT]
        if records:
            paths = list(dict.fromkeys(path for record in records for path in [record["path"], record.get("previous_path")] if path is not None))
            patch = git_bytes(base, *options, "-p", "--no-color", *revisions, "--", *(f":(literal){path}" for path in paths)).decode("utf-8", "replace")
            blocks = patch_blocks(patch)
            if len(blocks) != len(records):
                raise ValueError("git file list and patch blocks disagree")
            for record, block in zip(records, blocks):
                attach_patch(record, block)
    except subprocess.CalledProcessError as error:
        raise ValueError(error.stderr.decode("utf-8", "replace").strip() or "git commit lookup failed") from None
    return {"sha": commit_sha, "author": author, "date": date, "message": message.rstrip("\n"), "parents": parents, "files": records, "truncated": truncated}


def changed_paths(raw: bytes) -> list[dict]:
    """Never split or unquote a display header to recover a filename."""
    fields = raw.split(b"\0")
    if fields and not fields[-1]:
        fields.pop()
    records = []
    position = 0
    while position < len(fields):
        status = fields[position].decode("ascii")
        position += 1
        if position >= len(fields):
            raise ValueError("git name-status missing path")
        path = fields[position].decode("utf-8", "replace")
        position += 1
        record = {"path": path, "status": {"A": "added", "D": "removed"}.get(status[:1], "modified")}
        if status.startswith(("R", "C")):
            if position >= len(fields):
                raise ValueError("git rename missing destination")
            record["previous_path"] = path
            record["path"] = fields[position].decode("utf-8", "replace")
            record["status"] = "renamed" if status.startswith("R") else "added"
            position += 1
        records.append(record)
    return records


def patch_blocks(patch: str) -> list[list[str]]:
    blocks: list[list[str]] = []
    for line in patch.split("\n"):
        if line.startswith("diff --git "):
            blocks.append([])
        elif blocks:
            blocks[-1].append(line)
    return blocks


def attach_patch(record: dict, lines: list[str]) -> None:
    if lines and lines[-1] == "":
        lines.pop()  # patch transport terminator, not a source line
    binary = any(line.startswith("Binary files ") or line == "GIT binary patch" for line in lines)
    record["binary"] = binary
    if binary:
        return  # counts genuinely unknown; never label them zero
    start = next((index for index, line in enumerate(lines) if line.startswith("@@ ")), len(lines))
    hunks = lines[start:]
    record["additions"] = sum(line.startswith("+") for line in hunks)
    record["deletions"] = sum(line.startswith("-") for line in hunks)
    record["patch"] = "\n".join(hunks)
