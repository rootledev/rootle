#!/usr/bin/env python3
"""Build a stress-test fs root for breaking rootle (plans/0023 ralph
loops): `python3 e2e/stress_root.py /tmp/rootle-break-root`.

Deliberately nasty corpus for the fs provider: many repos (search
paging), deep nesting, unicode/spacey names, huge and tiny files,
binary + non-UTF8 bytes, CRLF and tab indent, long lines, empty
files/dirs, and one real git worktree with branches/tags/history for
the revision lenses. Deterministic: same tree every run.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

LANG_FILES = {
    "src/main.rs": 'fn main() {\n    println!("stress");\n}\n',
    "src/lib.rs": "pub fn render() -> &'static str {\n    \"rootle\"\n}\n",
    "app.py": "def main():\n    print('stress')\n",
    "notes.md": "# notes\n\nrender the thing\n",
    "config.toml": '[package]\nname = "stress"\n',
    "site.js": "const render = () => 'stress';\n",
}


def write(path: Path, data: str | bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(data, bytes):
        path.write_bytes(data)
    else:
        path.write_text(data)


def nasty_repo(root: Path, name: str) -> None:
    repo = root / name
    for rel, content in LANG_FILES.items():
        write(repo / rel, content.replace("stress", name))
    # unicode + spaces + long names
    write(repo / "docs/ümlaut ünïcode.md", "# ünïcode\n")
    write(repo / "docs/space in name.txt", "spaces\n")
    write(repo / ("docs/" + "a" * 120 + ".md"), "# long name\n")
    # huge file (10k lines), empty file, binary, non-UTF8, CRLF, tabs
    write(repo / "big/huge.rs", "".join(f"fn f{i}() {{}}\n" for i in range(10_000)))
    write(repo / "big/empty.rs", "")
    write(repo / "big/blob.bin", bytes(range(256)) * 16)
    write(repo / "big/not-utf8.rs", b"fn broken() {\n    \xff\xfe\x00bad\n}\n")
    write(repo / "big/crlf.rs", b"fn a() {}\r\nfn b() {}\r\n")
    write(repo / "big/tabs.rs", "fn tabbed() {\n\tlet x = 1;\n}\n")
    write(repo / "big/longline.rs", "fn long() {\n    let s = \"" + "x" * 500 + "\";\n}\n")
    (repo / "empty-dir").mkdir(parents=True, exist_ok=True)
    # deep nesting
    deep = repo
    for i in range(12):
        deep = deep / f"d{i}"
    write(deep / "leaf.rs", "fn leaf() {}\n")


def git_repo(root: Path) -> None:
    """A worktree with two branches, a tag, and multi-commit history."""
    repo = root / "proj"
    repo.mkdir(parents=True)
    env = {
        "GIT_AUTHOR_NAME": "Tarek",
        "GIT_AUTHOR_EMAIL": "t@example.com",
        "GIT_COMMITTER_NAME": "Tarek",
        "GIT_COMMITTER_EMAIL": "t@example.com",
        "GIT_AUTHOR_DATE": "2026-08-01T10:00:00Z",
        "GIT_COMMITTER_DATE": "2026-08-01T10:00:00Z",
        "PATH": "/usr/bin:/bin:/usr/local/bin",
        "HOME": str(root),
    }

    def git(*args: str) -> None:
        subprocess.run(["git", *args], cwd=repo, env=env, check=True, capture_output=True)

    git("init", "-b", "main")
    write(repo / "main.rs", 'fn main() {\n    println!("hi");\n}\n')
    git("add", ".")
    git("commit", "-m", "initial main.rs")
    env["GIT_AUTHOR_DATE"] = env["GIT_COMMITTER_DATE"] = "2026-08-02T10:00:00Z"
    write(repo / "main.rs", 'fn main() {\n    println!("hi");\n    println!("again");\n}\n')
    git("add", ".")
    git("commit", "-m", "second println")
    git("tag", "v1.0")
    git("checkout", "-b", "feature")
    write(repo / "main.rs", 'fn main() {\n    println!("hi");\n    println!("again");\n    println!("feature");\n}\n')
    git("add", ".")
    git("commit", "-m", "feature println")
    git("checkout", "main")


def main() -> None:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/rootle-break-root")
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)
    # 20 plain repos (search/list paging) + the nasty ones + git worktree
    for i in range(20):
        repo = root / f"repo{i:02}"
        for rel, content in LANG_FILES.items():
            write(repo / rel, content.replace("stress", f"repo{i:02}"))
    nasty_repo(root, "alpha")
    nasty_repo(root, "beta")
    git_repo(root)
    print(root)


if __name__ == "__main__":
    main()
