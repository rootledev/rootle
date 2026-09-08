"""Exercise CLI ownership and verified self-replacement without real installs."""
from __future__ import annotations

import hashlib
import io
import json
import platform
import shutil
import subprocess
import tarfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
from tui import hermetic_env


@pytest.fixture
def local_release(binary):
    current = subprocess.run([str(binary), '--version'], check=True, capture_output=True, text=True).stdout.split()[-1]
    version = f'{int(current.split(".")[0]) + 1}.0.0'
    architecture = 'aarch64' if platform.machine() in ('aarch64', 'arm64') else 'x86_64'
    target = f'{architecture}-apple-darwin' if platform.system() == 'Darwin' else f'{architecture}-unknown-linux-musl'
    filename = f'rootle-{version}-{target}.tar.gz'
    payload = f'#!/bin/sh\nprintf "rootle {version}\\n"\n'.encode()
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode='w:gz') as package:
        member = tarfile.TarInfo('package/rootle')
        member.size = len(payload)
        member.mode = 0o755
        package.addfile(member, io.BytesIO(payload))
    tarball = archive.getvalue()
    requests = []

    class ReleaseHandler(BaseHTTPRequestHandler):
        def log_message(self, *_arguments):
            pass

        def do_GET(self):
            requests.append(self.path)
            if self.path == '/repos/rootledev/rootle/releases/latest':
                body = json.dumps({'tag_name': f'v{version}', 'assets': [
                    {'name': filename, 'browser_download_url': f'{base}/{filename}'},
                    {'name': filename + '.sha256', 'browser_download_url': f'{base}/{filename}.sha256'},
                ]}).encode()
            elif self.path == '/' + filename:
                body = tarball
            elif self.path == '/' + filename + '.sha256':
                body = f'{hashlib.sha256(tarball).hexdigest()}  {filename}\n'.encode()
            else:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(('127.0.0.1', 0), ReleaseHandler)
    base = f'http://127.0.0.1:{server.server_port}'
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield base, requests, payload, version
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def update_environment(home: Path, base: str):
    home.mkdir()
    return hermetic_env(home, {
        'ROOTLE_UPDATE_API': base,
        'NO_PROXY': '127.0.0.1,localhost',
        'no_proxy': '127.0.0.1,localhost',
        'XDG_DATA_HOME': str(home / 'data'),
    })


@pytest.mark.parametrize('arguments', [('update', '--check'), ('--update', '--check')])
def test_update_routes_to_application_check(binary, tmp_path, local_release, arguments):
    base, requests, _payload, _version = local_release
    result = subprocess.run([str(binary), *arguments], env=update_environment(tmp_path / 'home', base),
                            capture_output=True, text=True, timeout=20)
    assert result.returncode == 0, result.stderr
    assert requests == ['/repos/rootledev/rootle/releases/latest']


@pytest.mark.parametrize('command,expected_status', [('self-update', 0), ('update', 1)])
def test_self_replacement_and_provider_sweep_ownership(binary, tmp_path, local_release, command, expected_status):
    base, _requests, payload, version = local_release
    executable = tmp_path / 'rootle'
    shutil.copy2(binary, executable)
    environment = update_environment(tmp_path / 'home', base)
    # A tracked provider cannot reach its API through the hermetic discard-port
    # proxy. Combined update reports that failure; self-update must not touch it.
    receipts = Path(environment['XDG_STATE_HOME']) / 'rootle' / 'providers'
    receipts.mkdir(parents=True)
    receipt = receipts / 'unreachable.toml'
    receipt.write_text('name = "unreachable"\nsource = "rootledev/rootle-unreachable"\n'
                       'tag = "v0.0.0"\nsha256 = "unused"\npinned = false\n')
    original_receipt = receipt.read_bytes()
    result = subprocess.run([str(executable), command], env=environment,
                            capture_output=True, text=True, timeout=20)
    assert result.returncode == expected_status, (result.stdout, result.stderr)
    assert executable.read_bytes() == payload
    assert receipt.read_bytes() == original_receipt
    assert subprocess.run([str(executable), '--version'], check=True, capture_output=True, text=True).stdout.strip() == f'rootle {version}'


def test_provider_update_does_not_check_application(binary, tmp_path, local_release):
    base, requests, _payload, _version = local_release
    result = subprocess.run([str(binary), 'provider', 'update'], env=update_environment(tmp_path / 'home', base),
                            capture_output=True, text=True, timeout=20)
    assert result.returncode == 0, result.stderr
    assert requests == []
