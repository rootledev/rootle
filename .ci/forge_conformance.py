#!/usr/bin/env python3
"""Run the real external forge suite at an explicit immutable commit."""
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def main() -> None:
    if len(sys.argv) != 2 or re.fullmatch(r'[0-9a-f]{40}', sys.argv[1]) is None:
        raise SystemExit('usage: forge_conformance.py <full immutable commit>')
    revision = sys.argv[1]
    root = Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory(prefix='rootle-forge-') as directory:
        checkout = Path(directory)
        subprocess.run(['git', 'init', '--quiet', str(checkout)], check=True)
        subprocess.run(['git', '-C', str(checkout), 'fetch', '--depth=1',
                        'https://github.com/rootledev/forge-conformance.git', revision], check=True)
        subprocess.run(['git', '-C', str(checkout), 'checkout', '--detach', 'FETCH_HEAD'], check=True)
        actual = subprocess.run(['git', '-C', str(checkout), 'rev-parse', 'HEAD'],
                                check=True, capture_output=True, text=True).stdout.strip()
        if actual != revision:
            raise RuntimeError(f'forge revision mismatch: {actual} != {revision}')
        print(f'forge-conformance input: {actual}', flush=True)
        subprocess.run([sys.executable, str(checkout / 'run'), '--', sys.executable,
                        str(root / 'crates/stdio/examples/fs_provider.py')], check=True)


if __name__ == '__main__':
    main()
