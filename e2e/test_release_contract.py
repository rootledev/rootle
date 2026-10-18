"""Pure artifact-contract regressions for the shared Actions/pilot qualifier."""
from __future__ import annotations

import hashlib
import importlib.util
from pathlib import Path

import pytest
from tui import ROOT

spec = importlib.util.spec_from_file_location('qualify_release', ROOT / '.ci' / 'qualify_release.py')
qualifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(qualifier)


def release_pair(directory: Path):
    package = 'rootle-0.12.2-x86_64-unknown-linux-musl'
    archive = directory / f'{package}.tar.gz'
    archive.write_bytes(b'contract checksum bytes')
    checksum = directory / f'{package}.tar.gz.sha256'
    checksum.write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
    return archive, checksum, package


def test_inventory_accepts_exact_tarball_and_sidecar(tmp_path):
    archive, _checksum, package = release_pair(tmp_path)
    assert qualifier.inventory(tmp_path, '0.12.2') == (archive, package)


@pytest.mark.parametrize('fault', ['extra', 'missing', 'checksum', 'sidecar-name', 'symlink'])
def test_inventory_rejects_every_contract_violation(tmp_path, fault):
    archive, checksum, _package = release_pair(tmp_path)
    if fault == 'extra':
        (tmp_path / 'rootle-0.12.2-aarch64-unknown-linux-musl.tar.gz').write_bytes(b'not supported')
    elif fault == 'missing':
        checksum.unlink()
    elif fault == 'checksum':
        archive.write_bytes(b'changed')
    elif fault == 'sidecar-name':
        checksum.write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  other.tar.gz\n')
    else:
        checksum.unlink()
        checksum.symlink_to(archive)
    with pytest.raises(ValueError):
        qualifier.inventory(tmp_path, '0.12.2')
