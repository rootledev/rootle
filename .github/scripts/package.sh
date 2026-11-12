#!/bin/sh
# Runs inside the selected native Linux image, including arm64 smoke execution.
set -eu
: "${VERSION:?release version required}"
: "${SOURCE_DATE_EPOCH:?source commit timestamp required}"
case "$(uname -m)" in
  x86_64) native=x86_64-unknown-linux-musl ;;
  aarch64) native=aarch64-unknown-linux-musl ;;
  *) echo 'unsupported Linux architecture' >&2; exit 1 ;;
esac
[ "${TARGET:-$native}" = "$native" ] || { echo 'target is not the native image architecture' >&2; exit 1; }
bin=/app/target/release/rootle
[ "$("$bin" --version)" = "rootle $VERSION" ]
python3 - "$VERSION" <<'PY'
import sys, tomllib
assert tomllib.load(open('Cargo.toml', 'rb'))['workspace']['package']['version'] == sys.argv[1]
PY
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
printf 'state\nkeys <esc><esc>\nframe\nstate\n' > "$work/smoke.txt"
HOME="$work" XDG_CONFIG_HOME="$work/config" XDG_STATE_HOME="$work/state" \
  "$bin" --headless "$work/smoke.txt" > "$work/smoke"
python3 - "$work/smoke" <<'PY'
import json, sys
states = [json.loads(line.removeprefix('─── state ')) for line in open(sys.argv[1]) if line.startswith('─── state ')]
assert [s['popup'] for s in states] == [True, False]
assert states[-1]['browser']['tree']['phase'] == 'idle'
PY
pkg=rootle-$VERSION-$native
mkdir -p "$work/$pkg" /dist
install -m755 "$bin" "$work/$pkg/rootle"
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
  -cf "$work/package.tar" -C "$work" "$pkg"
gzip -n -c "$work/package.tar" > "/dist/$pkg.tar.gz"
(cd /dist && sha256sum "$pkg.tar.gz" > "$pkg.tar.gz.sha256")
