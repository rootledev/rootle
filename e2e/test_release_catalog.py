"""Release inventory refuses missing architectures and altered build outputs."""
import hashlib
import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('release_catalog', ROOT / '.github/scripts/release-catalog.py')
CATALOG = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CATALOG)


def artifact(directory, target, payload=b'qualified artifact'):
    path = directory / f'rootle-1.2.3-{target}.tar.gz'
    path.write_bytes(payload)
    path.with_name(path.name + '.sha256').write_text(f'{hashlib.sha256(payload).hexdigest()}  {path.name}\n')
    return path


def test_catalog_requires_both_linux_architectures(tmp_path):
    artifact(tmp_path, 'x86_64-unknown-linux-musl')
    with pytest.raises(ValueError, match='both Linux'):
        CATALOG.build_catalog('v1.2.3', tmp_path, 'https://example.test', '2026-11-12T00:00:00Z')
    artifact(tmp_path, 'aarch64-unknown-linux-musl')
    catalog = CATALOG.build_catalog('v1.2.3', tmp_path, 'https://example.test', '2026-11-12T00:00:00Z')
    assert {entry['target'] for entry in catalog['artifacts']} == CATALOG.ARTIFACT_TARGETS
    assert all(entry['bytes'] == len(b'qualified artifact') for entry in catalog['artifacts'])


def test_catalog_rejects_tampered_artifacts(tmp_path):
    path = artifact(tmp_path, 'x86_64-unknown-linux-musl')
    path.write_bytes(b'tampered')
    with pytest.raises(ValueError, match='digest'):
        CATALOG.build_catalog('v1.2.3', tmp_path, 'https://example.test', '2026-11-12T00:00:00Z')


def test_catalog_rejects_foreign_versions(tmp_path):
    artifact(tmp_path, 'x86_64-unknown-linux-musl')
    with pytest.raises(ValueError, match='does not match tag'):
        CATALOG.build_catalog('v9.9.9', tmp_path, 'https://example.test', '2026-11-12T00:00:00Z')
