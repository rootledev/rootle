#!/bin/sh
# Qualification runs in the owned Buildbot; publishing is an explicit operator act.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"
exec python3 .github/scripts/release-local.py "$@"
