#!/bin/sh
# The `proofs` gate (Rootle request kernel), run at container start in the image
# proofs/Dockerfile builds: regenerate the translation and refuse drift
# against the committed proofs/Extracted/, build the proofs, audit every
# theorem's axioms, then calibrate with mutants that must fail.
set -eu
cd "$(dirname "$0")/.."
started="$(date +%s)"
stamp() { echo "[$(( $(date +%s) - started ))s] $*"; }

stamp "every kernel module is translated"
# Charon starts from [package.metadata.charon] start_from (module coverage): a
# module missing from it would ship unproved without any error.
python3 - <<'PY'
import re, tomllib
with open("crates/kernel/src/lib.rs") as handle:
    declared = re.findall(r"^\s*pub(?:\([^)]*\))?\s+mod\s+(\w+)\s*;", handle.read(), re.M)
with open("crates/kernel/Cargo.toml", "rb") as handle:
    listed = tomllib.load(handle)["package"]["metadata"]["charon"]["start_from"]
expected = {f"rootle_kernel::{module}" for module in declared}
missing, stale = sorted(expected - set(listed)), sorted(set(listed) - expected)
if missing or stale:
    raise SystemExit(f"FAIL: Charon start_from must name every `pub mod` in rootle-kernel's "
                     f"lib.rs; missing {missing}, not a module {stale}")
print(f"start_from covers {sorted(expected)}")
PY

stamp "extract: charon $(charon version) / aeneas $(aeneas -version)"
rm -rf /tmp/gate-extract
sh proofs/extract.sh crates/kernel /tmp/gate-extract

stamp "drift check against proofs/Extracted"
if ! diff -ru proofs/Extracted /tmp/gate-extract; then
    echo "FAIL: proofs/Extracted differs from a fresh translation of crates/kernel;" >&2
    echo "      run 'sh proofs/refresh.sh' and commit the result" >&2
    exit 1
fi

stamp "lake build"
cd proofs
lake build
stamp "axiom audit"
lake env lean Audit.lean
cd ..

stamp "mutant calibration"
python3 proofs/mutants.py

stamp "proofs gate: OK"
