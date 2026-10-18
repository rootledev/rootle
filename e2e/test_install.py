"""Installer policy, checksum integrity and real Linux CLI/PTY consumers."""
from __future__ import annotations

import hashlib
import os
import subprocess
import tarfile
import threading
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
from conftest import open_fs_repo
from headless import fs_config, run_headless, states
from tui import ROOT, Tui, hermetic_env


@pytest.mark.parametrize('system,machine', [
    ('Linux', 'aarch64'), ('Linux', 'arm64'), ('Linux', 'i686'),
    ('Linux', 'riscv64'), ('Darwin', 'x86_64'), ('Darwin', 'arm64'),
    ('MINGW64_NT', 'x86_64'), ('FreeBSD', 'amd64'),
])
def test_unsupported_installer_exits_before_network_or_mutation(tmp_path, system, machine):
    tools = tmp_path / 'tools'
    tools.mkdir()
    uname = tools / 'uname'
    uname.write_text(f'#!/bin/sh\ncase "$1" in -s) echo {system};; -m) echo {machine};; esac\n')
    uname.chmod(0o755)
    marker = tmp_path / 'called'
    for command in ('curl', 'wget', 'mktemp', 'mkdir', 'mv', 'chmod', 'tar'):
        tool = tools / command
        tool.write_text(f'#!/bin/sh\necho {command} >> "{marker}"\nexit 99\n')
        tool.chmod(0o755)
    home = tmp_path / 'home'
    env = hermetic_env(home, {'PATH': f'{tools}:{os.environ["PATH"]}'})
    env.pop('ROOTLE_VERSION', None)
    env['ROOTLE_INSTALL_DIR'] = str(home / 'bin')
    result = subprocess.run(['/bin/sh', str(ROOT / 'install.sh')], env=env,
                            capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert 'supports Linux/WSL x86_64 only' in result.stderr
    assert not marker.exists(), 'rejection must precede download, staging or install'
    assert not home.exists()


@pytest.fixture
def installer_release(tmp_path, binary):
    version = subprocess.run([str(binary), '--version'], check=True,
                             capture_output=True, text=True).stdout.split()[-1]
    package = f'rootle-{version}-x86_64-unknown-linux-musl'
    mirror = tmp_path / 'mirror'
    release = mirror / f'v{version}'
    release.mkdir(parents=True)
    archive = release / f'{package}.tar.gz'
    with tarfile.open(archive, 'w:gz') as output:
        output.add(binary, arcname=f'{package}/rootle')
    checksum = release / f'{package}.tar.gz.sha256'
    checksum.write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
    server = ThreadingHTTPServer(('127.0.0.1', 0),
                                partial(SimpleHTTPRequestHandler, directory=str(mirror)))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    home = tmp_path / 'home'
    home.mkdir()
    env = hermetic_env(home, {
        'ROOTLE_VERSION': version,
        'ROOTLE_BASE_URL': f'http://127.0.0.1:{server.server_port}',
        'ROOTLE_INSTALL_DIR': str(home / 'bin'),
        'ROOTLE_INSTALL_YES': '1',
        'NO_PROXY': '127.0.0.1,localhost',
        'no_proxy': '127.0.0.1,localhost',
        # BusyBox wget does not honor no_proxy on every Alpine version.
        'HTTP_PROXY': '', 'HTTPS_PROXY': '', 'http_proxy': '', 'https_proxy': '',
    })
    try:
        yield env, checksum, home / 'bin' / 'rootle'
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@pytest.mark.parametrize('fault', ['missing', 'mismatch'])
def test_installer_requires_valid_checksum(installer_release, fault):
    env, checksum, executable = installer_release
    if fault == 'missing':
        checksum.unlink()
    else:
        checksum.write_text(f'{"0" * 64}  {checksum.name.removesuffix(".sha256")}\n')
    result = subprocess.run(['/bin/sh', str(ROOT / 'install.sh')], env=env,
                            capture_output=True, text=True, timeout=30)
    assert result.returncode != 0
    assert 'checksum' in result.stderr
    assert not executable.parent.exists()


def test_installed_binary_runs_headless_and_pty(installer_release, binary, tmp_path):
    env, _checksum, executable = installer_release
    result = subprocess.run(['/bin/sh', str(ROOT / 'install.sh')], env=env,
                            capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, (result.stdout, result.stderr)
    assert executable.read_bytes() == binary.read_bytes()
    config = fs_config(tmp_path)
    output = run_headless(executable, 'state\nkeys <esc><esc>\nframe\nstate\n',
                          '--config', str(config), home=tmp_path / 'headless-home')
    assert [state['popup'] for state in states(output)] == [True, False]
    tui = Tui(executable, args=['--config', str(config)]).start()
    try:
        open_fs_repo(tui)
        tui.send('q')
        assert tui.wait_exit() == 0
        assert b'\x1b[?1049h' in tui.raw()
        assert b'\x1b[?1049l' in tui.raw()
    finally:
        tui.stop()
