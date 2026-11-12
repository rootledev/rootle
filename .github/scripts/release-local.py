#!/usr/bin/env python3
"""Linux release operator: qualify, package, draft, crates, publish, verify VERSION.

package is a nonpublishing Buildbot gate. Every publishing command independently
admits the full frozen Buildbot evidence, matches its source and profile to HEAD,
and copies only its inventoried artifacts. No local skip-gates receipt exists.
CI_INFRA_ROOT locates the trusted ci-infra installation; CI_STATE selects its state.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
os.chdir(ROOT)
REPO = 'rootledev/rootle'
TARGETS = {'linux/amd64': 'x86_64-unknown-linux-musl', 'linux/arm64': 'aarch64-unknown-linux-musl'}


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def text(*args):
    return run(*args, capture_output=True).stdout.strip()


def digest(path):
    with open(path, 'rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def source(version):
    if text('git', 'status', '--porcelain', '--untracked-files=all'):
        raise ValueError('release requires a clean checkout')
    if any(line[:1] == 'S' or line[:1].islower() for line in text('git', 'ls-files', '-v').splitlines()):
        raise ValueError('hidden index entries are forbidden')
    manifest = tomllib.loads(Path('Cargo.toml').read_text())
    if manifest['workspace']['package']['version'] != version:
        raise ValueError('workspace version does not match release')
    profile = json.loads(Path('.ci/pilot.json').read_text())
    if profile['version'] != version:
        raise ValueError('Buildbot profile version does not match release')
    return text('git', 'rev-parse', 'HEAD'), profile


def infra():
    sys.path.insert(0, os.environ.get('CI_INFRA_ROOT', '/home/tarek/workspace/ci-infra'))
    from ci_infra import records
    from ci_infra.bootstrap import configuration
    from ci_infra import client, storage
    state = Path(os.environ.get('CI_STATE', str(Path.home() / '.local/share/buildbot-ci')))
    storage.check(state)
    return records, configuration, client, state


def admitted(version, dist, candidate=None):
    head, profile = source(version)
    records, configuration, client, state = infra()
    settings = configuration(state)
    # A Git bundle is a serialization, not source identity: adding a tag or
    # repacking can change its bytes without changing HEAD. Reuse the snapshot
    # whose gates actually ran, then bind it independently to this checkout.
    if candidate is None:
        candidate = records.load_candidate(dist / 'candidate.json')
    frozen = state / 'inbox' / candidate['candidate_id']
    candidate = records.load_candidate(frozen / 'candidate.json')
    inputs = candidate['inputs']
    origins = text('git', 'config', '--get-all', 'remote.origin.url').splitlines()
    if (inputs['commit'] != head
            or inputs['tree'] != text('git', 'rev-parse', 'HEAD^{tree}')
            or inputs['profile'] != profile
            or inputs['repository'] != f'https://github.com/{REPO}.git'
            or len(origins) != 1
            or origins[0].removesuffix('.git') != inputs['repository'].removesuffix('.git')
            or inputs['worker_image'] != settings['image']
            or inputs['adapter_sha256'] != settings['adapter_sha256']):
        raise ValueError('candidate source or runtime mismatch')
    if records.sha256_file(frozen / 'source.bundle') != inputs['bundle_sha256']:
        raise ValueError('frozen source bundle changed')
    evidence = state / 'archive' / candidate['candidate_id'] / 'evidence'
    qualification = records.admit(candidate, evidence)
    dist.mkdir(parents=True, exist_ok=True)
    expected = set(profile['artifacts'])
    if expected != {a['source_path'] for a in qualification['artifacts']}:
        raise ValueError('qualification artifact coverage mismatch')
    for artifact in qualification['artifacts']:
        path = Path(artifact['source_path'])
        if path.parent != dist.relative_to(ROOT):
            raise ValueError('artifact outside release directory')
        origin = evidence / artifact['gate'] / artifact['attempt'] / 'artifacts' / path
        if digest(origin) != artifact['sha256']:
            raise ValueError('artifact changed after admission')
        shutil.copyfile(origin, ROOT / path)
    (dist / 'qualification.json').write_text(json.dumps(qualification, indent=2) + '\n')
    (dist / 'candidate.json').write_text(json.dumps(candidate, indent=2) + '\n')
    return head, [ROOT / a for a in sorted(expected)] + [dist / 'qualification.json', dist / 'candidate.json']


def tagged(head, tag):
    remote = text('git', 'ls-remote', 'origin', f'refs/tags/{tag}', f'refs/tags/{tag}^{{}}').splitlines()
    commits = {line.split()[1]: line.split()[0] for line in remote}
    if commits.get(f'refs/tags/{tag}^{{}}', commits.get(f'refs/tags/{tag}')) != head:
        raise ValueError('remote release tag must point to qualified HEAD')


def compare_remote(tag, files):
    with tempfile.TemporaryDirectory() as work:
        run('gh', 'release', 'download', tag, '--repo', REPO, '--dir', work)
        for path in files:
            if digest(Path(work) / path.name) != digest(path):
                raise ValueError(f'remote asset differs: {path.name}')


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ('qualify', 'package', 'draft', 'crates', 'publish', 'verify'):
        raise ValueError(__doc__)
    command, version = sys.argv[1], sys.argv[2].removeprefix('v')
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
        raise ValueError('release version must be a semver triple')
    tag = 'v' + version
    dist = ROOT / 'dist' / tag
    head, _ = source(version)
    if command == 'qualify':
        records, configuration, client, state = infra()
        settings = configuration(state)
        candidate = records.freeze(ROOT, state / 'inbox', settings['image'], settings['adapter_sha256'])
        print(json.dumps(client.submit(state, candidate['candidate_id']), indent=2), flush=True)
        env = dict(os.environ, PYTHONPATH=str(Path(os.environ.get('CI_INFRA_ROOT', '/home/tarek/workspace/ci-infra'))))
        run(sys.executable, '-m', 'ci_infra', '--state', str(state), 'watch', candidate['candidate_id'], env=env)
        admitted(version, dist, candidate)
        return
    if command == 'package':
        dist.mkdir(parents=True, exist_ok=True)
        epoch = text('git', 'show', '-s', '--format=%ct', 'HEAD')
        for platform, target in TARGETS.items():
            env = dict(os.environ, ROOTLE_PLATFORM=platform, ROOTLE_DIST=str(dist))
            run('docker', 'compose', 'run', '--build', '--rm', '-e', f'VERSION={version}',
                '-e', f'TARGET={target}', '-e', f'SOURCE_DATE_EPOCH={epoch}', 'release', env=env)
        run(sys.executable, '.github/scripts/release-catalog.py', 'catalog', '--tag', tag,
            '--dist', str(dist), '--out', str(dist / 'catalog.json'), '--published-at', text('git', 'show', '-s', '--format=%cI', 'HEAD'))
        return
    head, files = admitted(version, dist)
    tagged(head, tag)
    if command == 'draft':
        run('gh', 'release', 'create', tag, '--repo', REPO, '--verify-tag', '--draft',
            '--title', f'rootle {tag}', '--generate-notes', *(str(p) for p in files))
    elif command == 'crates':
        credentials = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo'))) / 'credentials.toml'
        if not credentials.is_file() and not os.environ.get('CARGO_REGISTRY_TOKEN'):
            raise ValueError('cargo publication requires credentials.toml or CARGO_REGISTRY_TOKEN')
        authentication = (['-v', f'{credentials}:/usr/local/cargo/credentials.toml:ro']
                          if credentials.is_file() else ['-e', 'CARGO_REGISTRY_TOKEN'])
        for crate in ('rootle-kernel', 'rootle-trace', 'rootle-provider', 'rootle-diff', 'rootle-stdio', 'rootle-github', 'rootle-manager', 'rootle'):
            run('docker', 'compose', 'run', '--build', '--rm', *authentication,
                '--entrypoint', 'cargo', 'release', 'publish', '--locked', '-p', crate)
    elif command == 'publish':
        if text('gh', 'release', 'view', tag, '--repo', REPO, '--json', 'isDraft', '--jq', '.isDraft') != 'true':
            raise ValueError('publication requires an existing draft')
        compare_remote(tag, files)
        run('gh', 'release', 'edit', tag, '--repo', REPO, '--draft=false', '--latest')
    else:
        compare_remote(tag, files)
        with tempfile.TemporaryDirectory() as work:
            env = dict(os.environ, ROOTLE_VERSION=version, ROOTLE_INSTALL_DIR=work, ROOTLE_INSTALL_YES='1')
            run('sh', 'install.sh', env=env)
            if text(str(Path(work) / 'rootle'), '--version') != f'rootle {version}':
                raise ValueError('installed version differs')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
