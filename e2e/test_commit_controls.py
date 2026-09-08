"""Real commit pane input ownership, tree navigation, and revision-aware yanks."""
from __future__ import annotations

import json
import os
import subprocess

from conftest import FS_PROVIDER
from headless import frames, run_headless, states


def commit_controls_fixture(tmp_path):
    root = tmp_path / 'code'
    repository = root / 'project'
    (repository / 'src' / 'deep').mkdir(parents=True)
    environment = {
        **os.environ, 'HOME': str(tmp_path), 'GIT_CONFIG_NOSYSTEM': '1',
        'GIT_AUTHOR_NAME': 'Commit Fixture', 'GIT_AUTHOR_EMAIL': 'fixture@example.test',
        'GIT_COMMITTER_NAME': 'Commit Fixture', 'GIT_COMMITTER_EMAIL': 'fixture@example.test',
        'GIT_AUTHOR_DATE': '2026-08-20T10:00:00Z', 'GIT_COMMITTER_DATE': '2026-08-20T10:00:00Z',
    }

    def git(*arguments):
        return subprocess.run(['git', '-C', str(repository), *arguments], env=environment,
                              check=True, capture_output=True, text=True).stdout.strip()

    git('init', '-b', 'main')
    source = repository / 'src' / 'deep' / 'alpha.rs'
    source.write_text('fn alpha() {\n    let old_value = 1;\n    let needle_one = 2;\n    let needle_two = 3;\n}\n')
    (repository / 'src' / 'beta.rs').write_text('fn beta() {}\n')
    (repository / 'z.txt').write_text('original root\n')
    git('add', '.')
    git('commit', '-qm', 'initial files')
    parent = git('rev-parse', 'HEAD')
    source.write_text('fn alpha() {\n    let new_value = 10;\n    let needle_one = 2;\n    let needle_two = 30;\n}\n')
    (repository / 'src' / 'beta.rs').write_text('fn beta_changed() {}\n')
    (repository / 'z.txt').write_text('changed root\n')
    git('add', '.')
    git('commit', '-qm', 'change nested files')
    revision = git('rev-parse', 'HEAD')
    # Extend the reference adapter only at its optional web-link boundary.
    # All repository/history/commit/patch data still comes from the real git worktree.
    provider = tmp_path / 'permalink_provider.py'
    provider.write_text(
        'import sys\nfrom urllib.parse import quote\n'
        f'sys.path.insert(0, {str(FS_PROVIDER.parent)!r})\n'
        'import fs_provider\noriginal = fs_provider.handle\n'
        'def handle(root, method, params):\n'
        '    if method == "repo/web_url":\n'
        '        url = "https://forge.example/" + quote(params["repo"], safe="/") + "/blob/" + quote(params.get("branch", ""), safe="") + "/" + quote(params.get("path", ""), safe="/")\n'
        '        if params.get("line"):\n'
        '            url += "#L" + str(params["line"])\n'
        '        return {"url": url}\n'
        '    result = original(root, method, params)\n'
        '    if method == "repo/commit":\n'
        '        result["web_url"] = "https://forge.example/" + params["repo"] + "/commit/" + result["sha"]\n'
        '    return result\n'
        'fs_provider.handle = handle\nfs_provider.main()\n'
    )
    config = tmp_path / 'config.toml'
    config.write_text('[provider]\nkind = "stdio"\ncommand = ' + json.dumps(['python3', str(provider), str(root)]) + '\n')
    return config, revision, parent


def test_diff_search_stays_in_preview_and_yanks_correct_revision(tmp_path, binary):
    config, revision, parent = commit_controls_fixture(tmp_path)
    output = run_headless(binary,
        'keys <space>h\nsettle\nkeys <cr>\nstate\n'
        'keys d\nsettle\nkeys y\nstate\nkeys <cr>\nframe\n'
        'keys /needle\nframe\nstate\nkeys <cr>y\nstate\n'
        'keys n\nkeys y\nstate\nkeys Y\nstate\n'
        'keys n\nkeys y\nstate\n'
        'keys /not-present\nframe\nkeys <esc>\nkeys y\nstate\n',
        'local/project', '--config', str(config), home=tmp_path / 'home', cols=160, rows=30)
    recorded = states(output)
    assert recorded[0]['mode'] == 'HISTORY'  # Enter no longer duplicates d.
    assert recorded[1]['yanks'][-1] == f'https://forge.example/local/project/commit/{revision}'
    assert recorded[2]['mode'] == 'FIND'
    assert recorded[2]['surface']['delta'] is not None
    assert recorded[3]['yanks'][-1].endswith(f'/blob/{revision}/src/deep/alpha.rs#L3')
    assert recorded[4]['yanks'][-1].endswith(f'/blob/{parent}/src/deep/alpha.rs#L4')
    assert recorded[5]['yanks'][-1] == '    let needle_two = 3;\n'
    assert recorded[6]['yanks'][-1].endswith(f'/blob/{revision}/src/deep/alpha.rs#L4')
    assert recorded[7]['yanks'][-1] == recorded[6]['yanks'][-1]  # canceled query restores cursor
    opened, searching, missing = frames(output)
    assert 'src/' in opened and 'deep/' in opened and 'alpha.rs' in opened
    assert 'files (3)' in searching and '/needle · 1/3' in searching
    assert '0/0' in missing and 'files (3)' in missing


def test_file_filter_and_search_have_independent_escape_ladders(tmp_path, binary):
    config, _revision, _parent = commit_controls_fixture(tmp_path)
    output = run_headless(binary,
        'keys <space>h\nsettle\nkeys d\nsettle\nkeys <cr><tab>\n'
        'keys /beta<cr>\nkeys ]f\nframe\n'
        'keys <tab>/beta<cr>\nkeys Y\nstate\n'
        'keys <esc>\nstate\nkeys <esc>\nstate\nkeys <esc>\nstate\n'
        'keys <esc>\nstate\nkeys <esc>\nstate\n',
        'local/project', '--config', str(config), home=tmp_path / 'home', cols=160)
    assert 'beta.rs' in frames(output)[0] and 'alpha.rs' not in frames(output)[0]
    recorded = states(output)
    assert recorded[0]['yanks'][-1] == 'fn beta() {}\n'
    assert recorded[1]['surface']['delta'] is not None  # clear find, not file filter
    assert recorded[2]['surface']['delta'] is None  # close diff
    assert recorded[3]['mode'] == 'COMMIT'  # clear file filter
    assert recorded[4]['mode'] == 'HISTORY'
    assert recorded[5]['mode'] == 'BROWSE'
