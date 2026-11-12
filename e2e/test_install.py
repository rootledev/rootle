"""Installer platform and mandatory-integrity contract, without public downloads."""
from pathlib import Path
import hashlib
import io
import os
import subprocess
import tarfile

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.parametrize('architecture,target', [
    ('x86_64', 'x86_64-unknown-linux-musl'),
    ('aarch64', 'aarch64-unknown-linux-musl'),
])
@pytest.mark.parametrize('checksum', ['valid', 'missing', 'tampered'])
def test_linux_install_requires_checksum(tmp_path, architecture, target, checksum):
    tools = tmp_path / 'tools'
    tools.mkdir()
    archive = f'rootle-1.2.3-{target}.tar.gz'
    data = io.BytesIO()
    payload = b'#!/bin/sh\necho rootle 1.2.3\n'
    with tarfile.open(fileobj=data, mode='w:gz') as package:
        entry = tarfile.TarInfo(f'rootle-1.2.3-{target}/rootle')
        entry.size = len(payload)
        entry.mode = 0o755
        package.addfile(entry, io.BytesIO(payload))
    (tmp_path / archive).write_bytes(data.getvalue())
    if checksum != 'missing':
        digest = hashlib.sha256(data.getvalue()).hexdigest() if checksum == 'valid' else '0' * 64
        (tmp_path / (archive + '.sha256')).write_text(f'{digest}  {archive}\n')
    (tools / 'uname').write_text(f'#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo {architecture};; esac\n')
    (tools / 'curl').write_text('#!/bin/sh\n# fetch_to: curl -fsSL -o DEST URL\ncp "$FIXTURES/${4##*/}" "$3"\n')
    for path in tools.iterdir():
        path.chmod(0o755)
    destination = tmp_path / 'bin'
    env = dict(os.environ, PATH=f'{tools}:{os.environ["PATH"]}', FIXTURES=str(tmp_path),
               ROOTLE_VERSION='1.2.3', ROOTLE_INSTALL_DIR=str(destination), ROOTLE_INSTALL_YES='1')
    result = subprocess.run(['sh', str(ROOT / 'install.sh')], env=env, capture_output=True, text=True)
    assert (result.returncode == 0) == (checksum == 'valid'), (result.stdout, result.stderr)
    assert (destination / 'rootle').exists() == (checksum == 'valid')


def test_installer_rejects_non_linux(tmp_path):
    tools = tmp_path / 'tools'
    tools.mkdir()
    uname = tools / 'uname'
    uname.write_text('#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo arm64;; esac\n')
    uname.chmod(0o755)
    env = dict(os.environ, PATH=f'{tools}:{os.environ["PATH"]}', ROOTLE_VERSION='1.2.3', ROOTLE_INSTALL_YES='1')
    result = subprocess.run(['sh', str(ROOT / 'install.sh')], env=env, capture_output=True, text=True)
    assert result.returncode != 0
    assert 'supports Linux' in result.stderr
