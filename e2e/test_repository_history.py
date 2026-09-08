"""Repository history and shared commit sidebar through real provider IPC."""
from __future__ import annotations

import os
import subprocess

from conftest import make_git_root
from headless import fs_config, frames, run_headless, states


def history_fixture(tmp_path):
    root = make_git_root(tmp_path)
    repository = root / 'proj'
    environment = {
        **os.environ,
        'HOME': str(tmp_path),
        'GIT_AUTHOR_NAME': 'History Fixture',
        'GIT_AUTHOR_EMAIL': 'history@example.test',
        'GIT_COMMITTER_NAME': 'History Fixture',
        'GIT_COMMITTER_EMAIL': 'history@example.test',
        'GIT_AUTHOR_DATE': '2026-08-20T10:00:00Z',
        'GIT_COMMITTER_DATE': '2026-08-20T10:00:00Z',
    }

    def git(*arguments):
        subprocess.run(['git', '-C', str(repository), *arguments], env=environment,
                       check=True, capture_output=True)

    (repository / 'nested').mkdir()
    (repository / 'nested' / 'only.txt').write_text('directory selection\n')
    (repository / 'notes.txt').write_text('old notes content\n')
    git('add', '.')
    git('commit', '-qm', 'docs only')
    (repository / 'main.rs').write_text('fn main() {\n    let changed = 2;\n}\n')
    (repository / 'notes.txt').write_text('new notes content\n')
    git('add', '.')
    git('commit', '-qm', 'change both files')
    return fs_config(tmp_path, root)


def test_repository_history_from_directory_and_sidebar_navigation(tmp_path, binary):
    config = history_fixture(tmp_path)
    output = run_headless(
        binary,
        'frame\nkeys <space>h\nsettle\nframe\n'
        'keys d\nsettle\nframe\nkeys <cr>\nframe\n'
        'keys <tab>j\nframe\nstate\n',
        'local/proj', '--config', str(config), home=tmp_path / 'home', cols=150,
    )
    initial, history, message, first_diff, second_diff = frames(output)
    assert 'nested/' in initial and 'fn main()' not in initial
    assert 'repo history' in history and 'docs only' in history
    assert 'change both files' in message and 'files (2)' in message
    assert 'let changed = 2' in first_diff and 'files (2)' in first_diff
    assert 'new notes content' in second_diff and 'files (2)' in second_diff
    assert states(output)[0]['mode'] == 'COMMIT'


def test_file_history_excludes_other_files_and_repo_history_honors_ref(tmp_path, binary):
    config = history_fixture(tmp_path)
    file_output = run_headless(
        binary, 'keys /main.rs<cr>\nsettle\nkeys <space>ph\nsettle\nframe\n',
        'local/proj', '--config', str(config), home=tmp_path / 'file-home', cols=150,
    )
    assert 'initial main.rs' in file_output and 'change both files' in file_output
    assert 'docs only' not in file_output
    branch_output = run_headless(
        binary, 'keys <space>h\nsettle\nframe\n',
        'local/proj@feature', '--config', str(config), home=tmp_path / 'branch-home', cols=150,
    )
    assert 'feature: print hi' in branch_output
    assert 'change both files' not in branch_output and 'docs only' not in branch_output
