#!/usr/bin/env python3
"""Exact Linux x64 artifact contract, shared by Actions and the local pilot.

No publishing: --consumer installs the original archive through install.sh and
runs the full real-binary headless/PTY suite with a disposable HOME.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import threading
import tomllib
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parent.parent
TARGET = 'x86_64-unknown-linux-musl'


def run(argv, **kwargs):
    return subprocess.run(argv, check=True, text=True, **kwargs)


def inventory(dist: Path, version: str) -> tuple[Path, str]:
    package = f'rootle-{version}-{TARGET}'
    name = f'{package}.tar.gz'
    expected = {name, name + '.sha256'}
    actual = {path.name for path in dist.iterdir()}
    if actual != expected:
        raise ValueError(f'exact artifact inventory: expected {expected}, got {actual}')
    for path in dist.iterdir():
        if not path.is_file() or path.is_symlink():
            raise ValueError(f'not a regular artifact: {path}')
    archive = dist / name
    checksum = (dist / (name + '.sha256')).read_text().split()
    if len(checksum) != 2 or checksum[1] != name:
        raise ValueError('checksum sidecar must name exactly the release tarball')
    with archive.open('rb') as source:
        actual_hash = hashlib.file_digest(source, 'sha256').hexdigest()
    if checksum[0] != actual_hash:
        raise ValueError('release checksum mismatch')
    print(f'verified inventory and sha256: {name}', flush=True)
    return archive, package


def environment(home: Path) -> dict[str, str]:
    env = dict(os.environ)
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / 'config'),
               XDG_CACHE_HOME=str(home / 'cache'), XDG_STATE_HOME=str(home / 'state'),
               XDG_DATA_HOME=str(home / 'data'), CI='true',
               HTTP_PROXY='http://127.0.0.1:9', HTTPS_PROXY='http://127.0.0.1:9',
               http_proxy='http://127.0.0.1:9', https_proxy='http://127.0.0.1:9')
    for key in ('ROOTLE_CONFIG', 'ROOTLE_TRACE', 'ROOTLE_UPDATE_API'):
        env.pop(key, None)
    return env


def verify_binary(archive: Path, package: str, version: str, scratch: Path) -> None:
    # Inspect before extraction: release packaging allows one directory and binary.
    with tarfile.open(archive, 'r:gz') as source:
        members = source.getmembers()
        binaries = [member for member in members if member.name == f'{package}/rootle' and member.isfile()]
        if len(binaries) != 1 or any(
            not ((member.name.rstrip('/') == package and member.isdir()) or member is binaries[0])
            for member in members
        ):
            raise ValueError('tarball must contain only the package directory and regular rootle binary')
        source.extractall(scratch, filter='data')
    binary = scratch / package / 'rootle'
    description = run(['file', str(binary)], capture_output=True).stdout
    if 'x86-64' not in description or not any(text in description for text in ('static-pie linked', 'statically linked')):
        raise ValueError(f'expected static x86_64 ELF: {description}')
    dynamic = run(['readelf', '-d', str(binary)], capture_output=True).stdout
    if '(NEEDED)' in dynamic:
        raise ValueError(f'ELF shared-library dependencies: {dynamic}')
    home = scratch / 'smoke-home'
    home.mkdir()
    env = environment(home)
    observed = run([str(binary), '--version'], env=env, capture_output=True).stdout.strip()
    if observed != f'rootle {version}':
        raise ValueError(f'package version mismatch: {observed}')
    smoke = run([str(binary), '--headless', '-'], env=env,
                input='state\nkeys <esc><esc>\nframe\nstate\n', capture_output=True).stdout
    states = [json.loads(line.removeprefix('─── state '))
              for line in smoke.splitlines() if line.startswith('─── state ')]
    if [state['popup'] for state in states] != [True, False] or states[-1]['browser']['tree']['phase'] != 'idle':
        raise ValueError(f'headless popup lifecycle failed: {states}')
    print(f'verified static ELF, version and headless lifecycle: {binary.name}', flush=True)


def consume(archive: Path, version: str, scratch: Path) -> None:
    mirror = scratch / 'mirror'
    release = mirror / f'v{version}'
    release.mkdir(parents=True)
    # Serve the original qualified bytes and sidecar, never a rebuilt substitute.
    shutil.copyfile(archive, release / archive.name)
    shutil.copyfile(Path(str(archive) + '.sha256'), release / (archive.name + '.sha256'))
    server = ThreadingHTTPServer(('127.0.0.1', 0),
                                partial(SimpleHTTPRequestHandler, directory=str(mirror)))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    home = scratch / 'consumer-home'
    home.mkdir()
    env = environment(home)
    env.update(ROOTLE_VERSION=version, ROOTLE_INSTALL_YES='1',
               ROOTLE_INSTALL_DIR=str(home / 'bin'),
               ROOTLE_BASE_URL=f'http://127.0.0.1:{server.server_port}',
               NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost',
               HTTP_PROXY='', HTTPS_PROXY='', http_proxy='', https_proxy='')
    try:
        run(['/bin/sh', str(ROOT / 'install.sh')], env=env)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    env = environment(home)
    env['ROOTLE_E2E_BINARY'] = str(home / 'bin' / 'rootle')
    run(['uv', 'run', '--locked', '--no-sync', 'pytest'], cwd=ROOT / 'e2e', env=env)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dist', type=Path, default=ROOT / 'dist')
    parser.add_argument('--version')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--inventory-only', action='store_true')
    mode.add_argument('--consumer', action='store_true')
    args = parser.parse_args()
    with (ROOT / 'Cargo.toml').open('rb') as manifest:
        workspace_version = tomllib.load(manifest)['workspace']['package']['version']
    version = args.version or workspace_version
    if version != workspace_version:
        raise ValueError(f'candidate/tag version {version} != workspace version {workspace_version}')
    archive, package = inventory(args.dist.resolve(), version)
    if not args.inventory_only:
        with tempfile.TemporaryDirectory(prefix='rootle-artifact-') as temp:
            scratch = Path(temp)
            verify_binary(archive, package, version, scratch)
            if args.consumer:
                consume(archive, version, scratch)


if __name__ == '__main__':
    main()
