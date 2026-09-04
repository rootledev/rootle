# 0023 — Headless driver, macOS behavioral CI, verified release

Status: **done (2026-09-04)** — M1 `--headless` driver + headless e2e
tier, M2 release headless smoke + provenance attestation, M3 macOS
behavioral CI, M4 docs/site changelog, M5 released as v0.9.0 (#135,
#136; attestation rootledev/rootle/attestations/45256396, tap CI green
on ubuntu+macOS, site live at rootle.dev/changelog/#090). The first
e2e-macos run caught three real defects, fixed in the same PR: XDG
base dirs were ignored on macOS (new `src/paths.rs`), the e2e sandbox's
network isolation was convention not enforcement (discard-port proxy
in `hermetic_env`), and two PTY tests expected states a fast tick
never draws.

## Problem

1. Rootle can only be driven through a real terminal. Every behavioral
   check — CI, reviewer, agent — goes through the PTY harness
   (`e2e/tui.py`: `pty.openpty` + pyte screen reconstruction + settle
   heuristics). Strop's postmortem (plans/0006 there) names the flake
   classes: timing, VT-emulation fidelity, input-byte ambiguity — "the
   app under test was never wrong often; the observation channel was."
2. CI is linux-only outside release builds. The release matrix already
   builds on `macos-14`, but no behavioral test ever runs on macOS —
   APFS case-insensitivity, symlink topology, and the crossterm/macOS
   edge cases ship untested (gripsack 0.20.0 changelog: "macOS
   behavioral CI — the full suite now runs natively every push").
3. Release integrity is sha256 sidecars only. Gripsack adds GitHub
   build-provenance attestations (`gh attestation verify <tarball>`)
   and an in-binary SBOM audit; rootle has neither.

## Research findings

### strop — `--headless` (crates/strop/src/headless.rs, ~100 lines)

- CLI: `strop --headless [script-file] [path]` — deterministic: no
  PTY, no timing. One input path: TUI and headless both call
  `editor.feed`; frames render to ratatui's `TestBackend`.
- Script language, one step per line: `keys <text>` (token forms
  `<esc>` `<cr>` `<bs>`), `wait <ms>` (wall-clock drain for real data
  sources), `settle` (drain until quiescent, bounded), `frame` (dump
  the cell grid), `state` (JSON: mode/cursor/pending/picker/…),
  `#` comments.
- In-crate `#[cfg(test)]` tests call `headless::frame_string`
  directly — 11 tests, zero PTY anywhere in the repo. The release
  workflow smoke-tests every shipped tarball headless:
  `printf 'frame\n' > smoke.txt; "$bin" --headless smoke.txt f.rs | grep -q 'fn main'`.
- `demos/scripts/feel.txt` dry-runs the first demo.tape section as a
  headless script — agents read frames, no GIF rendering needed.

### gripsack — macOS e2e + verified release

- `e2e-macos` CI job on `macos-14`: native (not docker — "macOS
  runners have no compose story worth the latency"), `cargo build` +
  `python -m pytest`. Binary path is repo-root-relative
  (cwd-relative broke the macOS job). Offline only: "network in e2e is
  a bug."
- Release attestations: `permissions: id-token: write,
  attestations: write` + `actions/attest-build-provenance@v2` over
  `dist/*.tar.gz` — verifiable with `gh attestation verify <file> -R
  gripsack-dev/gripsack`. No cosign; checksums stay.
- SBOM: `cargo auditable` builds + `cargo audit bin` on the shipped,
  stripped binary in the verify step (fails the release on advisories).

### rootle today

- `App::handle_key(KeyEvent)` is already the single input path;
  `App::with(State, tx)` constructs without a TTY; `tests/render.rs`
  (55 tests) renders to `TestBackend` offline. The headless seam
  exists — only the CLI driver + script language are missing.
- e2e: `tui.py` (262 ln) + `conftest.py` (127 ln) + 11 test files,
  offline via `examples/providers/fs_provider.py`, dockerized.
- release.yml: 4-target matrix (2 musl via docker, 2 darwin on
  macos-14), sha256 sidecars, `--version` smoke, crates.io → GitHub
  release → homebrew tap bump → site redeploy ping. No attestation.
- ci.yml: `test` (docker fmt+clippy+test, then docker e2e) +
  `forge-conformance`. No macOS job.

## Milestones

### M1 — `rootle --headless SCRIPT`

New `src/headless.rs` (lib side, beside `app/`): the script
interpreter + `frame_string` + `state_json`, mirroring strop's
language with rootle's nouns:

- `keys <text>` — token forms `<esc>` `<cr>` `<bs>` `<tab>` `<space>`
  `<up|down|left|right>`; chars feed through the same
  `App::handle_key` the TUI uses, then worker events drain.
- `settle` — drain the app-event channel to quiescence (bounded, 2s).
- `wait <ms>` — wall-clock drain (real providers reply on their own
  clock; strop bent "zero timing" the same way).
- `frame` — render to `TestBackend`, print the cell grid with a
  `─── frame COLS×ROWS` banner.
- `state` — one JSON line: mode, focused pane, popup, selection
  paths, pending keys, status/degraded lines, provider name.
- `#` comments; blank lines skipped; quit stops the driver.

CLI: `--headless <SCRIPT>` (`-` = stdin), composes with `--config` /
`--theme` / `owner/repo`; cols/rows via `ROOTLE_HEADLESS_COLS` /
`ROOTLE_HEADLESS_ROWS` (default 100×30 — strop hardcodes; make ours
overridable for pane-layout stress). No raw mode, no alternate screen,
no signal hooks, no editor suspend (an editor job is recorded into
`state`, not run), clipboard yanks recorded not written.

Tests: in-crate `#[cfg(test)]` coverage of the driver (offline
provider) + `e2e/test_headless.py` driving the real binary against
`fs_provider.py` — proving the same flow the PTY suite covers, without
the PTY.

### M2 — release: headless smoke + provenance attestation

- Every build-matrix verify step gains a headless smoke: script that
  opens the offline-org launch screen and greps a known cell
  (strop's release pattern; proves the binary *runs*, not just links).
- Release job: `permissions: + id-token: write, attestations: write`
  and `actions/attest-build-provenance@v2` with
  `subject-path: dist/rootle-*.tar.gz` (gripsack pattern; checksums
  stay, no cosign). SBOM/`cargo auditable` deferred — noted, not
  scope.

### M3 — macOS behavioral CI

New `e2e-macos` job in ci.yml on `macos-14`, native (gripsack's
reasoning: no compose story worth the latency):

1. `cargo build` (debug).
2. Headless suite: `e2e/test_headless.py` via pytest — deterministic
   even under runner timing noise.
3. Full PTY suite (`uv run pytest`) — pyte/`pty.openpty` are POSIX and
   gripsack proves native pytest-on-macOS works; catches
   darwin-specific terminal behavior the docker gate can't see.

Hermetic fixtures already redirect HOME/XDG; the binary path must be
repo-root-relative (gripsack's cwd-relative macOS breakage).

### M4 — docs + site changelog

- `doc/development.md` + AGENTS.md: the three tiers (in-crate frames,
  `--headless` scripts, PTY e2e) and which tier new tests belong in —
  PTY only for what a PTY proves (byte parsing, terminal restore).
- Site repo: `changelog` docs page sourced from the app repo's
  `CHANGELOG.md` at build time (pages.yml already checks out `code/`
  beside the site — zero manual mirroring on future releases).

### M5 — release v0.9.0

Minor bump (new `--headless` surface): Cargo.toml + CHANGELOG.md,
tag `v0.9.0`, watch the pipeline end to end: release workflow
(attestation present, 4 tarballs, crates.io, GitHub release),
homebrew-tap bump + tap CI green (style/audit/source-build/test on
ubuntu+macOS), site redeploy with the changelog page live.

## Deliberately not doing

- No PTY-suite rewrite. The 11-file e2e suite stays — it exercises the
  real byte path, terminal restore, and provider subprocesses. Strop's
  tiering applies to *new* tests: default to headless, PTY only for
  what needs a terminal.
- No cosign/keyless signing, no SBOM this round (attestation is the
  verified-release core; SBOM is a follow-up).
- No Windows anything (WSL is the story, unchanged).
