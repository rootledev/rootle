"""Release admission survives Git repacking, but never a source/runtime change.

The ledger fixture stands in for the separate ci-infra receipt validator. Git
objects, retained snapshots, artifact bytes and the release operator are real.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / '.github/scripts/release-local.py'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def candidate_for(inputs):
    encoded = json.dumps(inputs, sort_keys=True, separators=(',', ':')).encode()
    return {'schema': 1, 'candidate_id': hashlib.sha256(encoded).hexdigest(), 'inputs': inputs}


def git(repository, *arguments):
    return subprocess.check_output(
        ['git', '-C', str(repository), *arguments], text=True,
        env={**os.environ, 'GIT_CONFIG_GLOBAL': os.devnull, 'GIT_CONFIG_NOSYSTEM': '1'},
    ).strip()


class RetainedLedger:
    @staticmethod
    def load_candidate(path):
        candidate = json.loads(path.read_text())
        if candidate != candidate_for(candidate['inputs']):
            raise ValueError('candidate identity mismatch')
        return candidate

    sha256_file = staticmethod(digest)

    @staticmethod
    def admit(candidate, evidence):
        record = evidence / 'qualified.json'
        if not record.is_file():
            raise ValueError('missing passed gate: package')
        qualification = json.loads(record.read_text())
        if qualification['candidate_id'] != candidate['candidate_id']:
            raise ValueError('qualification identity mismatch')
        return qualification

    @staticmethod
    def freeze(repository, inbox, image, adapter):
        # A real fresh serialization, so reverting to re-freezing admission
        # selects a different unqualified snapshot after the repack below.
        bundle = inbox / 'fresh.bundle'
        git(repository, 'bundle', 'create', '--version=2', str(bundle), 'HEAD')
        inputs = {
            'repository': 'https://github.com/rootledev/rootle.git',
            'commit': git(repository, 'rev-parse', 'HEAD'),
            'tree': git(repository, 'rev-parse', 'HEAD^{tree}'),
            'bundle_sha256': digest(bundle),
            'profile': json.loads((repository / '.ci/pilot.json').read_text()),
            'worker_image': image, 'adapter_sha256': adapter,
            'policy': 'fixture',
        }
        candidate = candidate_for(inputs)
        frozen = inbox / candidate['candidate_id']
        frozen.mkdir(exist_ok=True)
        (frozen / 'candidate.json').write_text(json.dumps(candidate))
        (frozen / 'source.bundle').write_bytes(bundle.read_bytes())
        bundle.unlink()
        return candidate


@pytest.fixture
def release(tmp_path, monkeypatch):
    repository = tmp_path / 'repository'
    repository.mkdir()
    (repository / '.ci').mkdir()
    (repository / '.gitignore').write_text('dist/\n')
    (repository / 'Cargo.toml').write_text('[workspace.package]\nversion = "1.2.3"\n')
    (repository / 'source.txt').write_text('qualified source\n' * 5000)
    artifact = 'dist/v1.2.3/rootle-1.2.3-x86_64-unknown-linux-musl.tar.gz'
    profile = {'version': '1.2.3', 'artifacts': [artifact]}
    (repository / '.ci/pilot.json').write_text(json.dumps(profile))
    git(repository, 'init', '-q', '-b', 'main')
    git(repository, 'config', 'user.name', 'Release Fixture')
    git(repository, 'config', 'user.email', 'release@example.test')
    git(repository, 'remote', 'add', 'origin', 'https://github.com/rootledev/rootle.git')
    git(repository, 'add', '.')
    git(repository, 'commit', '-qm', 'qualified source')
    state = tmp_path / 'state'
    inbox = state / 'inbox'
    inbox.mkdir(parents=True)
    settings = {'image': 'sha256:' + '1' * 64, 'adapter_sha256': '2' * 64}
    candidate = RetainedLedger.freeze(repository, inbox, settings['image'], settings['adapter_sha256'])
    evidence = state / 'archive' / candidate['candidate_id'] / 'evidence'
    origin = evidence / 'package' / 'passed' / 'artifacts' / artifact
    origin.parent.mkdir(parents=True)
    origin.write_bytes(b'qualified executable artifact')
    qualification = {'candidate_id': candidate['candidate_id'], 'artifacts': [{
        'source_path': artifact, 'sha256': digest(origin), 'gate': 'package', 'attempt': 'passed',
    }]}
    (evidence / 'qualified.json').write_text(json.dumps(qualification))
    # Importing the operator changes cwd; contain it in this fixture's undo scope.
    monkeypatch.chdir(tmp_path)
    spec = importlib.util.spec_from_file_location('release_operator_fixture', SCRIPT)
    operator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(operator)
    monkeypatch.setattr(operator, 'ROOT', repository)
    monkeypatch.chdir(repository)
    monkeypatch.setattr(operator, 'infra', lambda: (RetainedLedger, lambda _: settings, None, state))
    directory = repository / 'dist/v1.2.3'
    operator.admitted('1.2.3', directory, candidate)
    return operator, repository, directory, state, settings, candidate


def test_tagging_and_repacking_keep_the_qualified_artifact(release):
    operator, repository, directory, state, settings, candidate = release
    git(repository, 'tag', '-a', 'v1.2.3', '-m', 'release')
    git(repository, '-c', 'core.compression=0', 'repack', '-adf')
    fresh = RetainedLedger.freeze(repository, state / 'inbox', settings['image'], settings['adapter_sha256'])
    assert fresh['inputs']['commit'] == candidate['inputs']['commit']
    assert fresh['inputs']['bundle_sha256'] != candidate['inputs']['bundle_sha256']
    head, files = operator.admitted('1.2.3', directory)
    assert head == candidate['inputs']['commit']
    archive = next(path for path in files if path.name.endswith('.tar.gz'))
    assert archive.read_bytes() == b'qualified executable artifact'
    assert json.loads((directory / 'candidate.json').read_text()) == candidate


@pytest.mark.parametrize('changed', ['source', 'runtime', 'bundle'])
def test_changed_release_inputs_never_reuse_qualified_artifacts(release, changed):
    operator, repository, directory, state, settings, candidate = release
    if changed == 'source':
        (repository / 'source.txt').write_text('different source')
        git(repository, 'add', 'source.txt')
        git(repository, 'commit', '-qm', 'different source')
    elif changed == 'runtime':
        settings['image'] = 'sha256:' + '3' * 64
    else:
        (state / 'inbox' / candidate['candidate_id'] / 'source.bundle').write_bytes(b'corrupt bundle')
    with pytest.raises(ValueError, match='mismatch|bundle changed'):
        operator.admitted('1.2.3', directory)
