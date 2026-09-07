# 0029 — Complete the polish wave and release it

Status: **done — released as v0.10.0 (2026-09-07)**. Implementation
merged in [#145](https://github.com/rootledev/rootle/pull/145), with
Linux/macOS CI green. Four platform artifacts and all six crates are
published; checksums, provenance, installed behavior, Homebrew and the
live site are verified below. This closes the earlier list-migration
deferral without calling bounded model checking an unbounded proof.

## Acceptance

- One shared list/filter/viewport implementation serves refs, help,
  settings, clone repos/destinations, history, and commit files. Entry
  selection is not a display-line offset: section headers, multi-line
  entries, empty filters, and small/resized viewports remain correct.
  `/` narrows incrementally; Enter commits; Esc restores, then clears,
  then dismisses. Preserve stable selected items across data refreshes.
- Keymap definitions supply dispatch and help, not parallel hand-written
  lists and matches. Stateful motion parsing consumes typed commands
  from those definitions. `]f` and `[f` are actual sequences.
- The commit viewer exposes the full message and files, keeps the
  selected line visible, handles binary/unavailable/truncated patches
  honestly, respects theme borders/palettes, sanitizes display text
  without corrupting opaque identifiers, and rejects stale results by
  repository plus request identity. Parsing and view changes happen in
  updates, never during draw. Small screens and Unicode must not panic.
- The protocol model matches advisory cancellation (a reply may still
  complete normally), opt-in partials, inactivity deadlines, validated
  restart, and request correlation. Safety bounds are not called a
  liveness proof. Temporal claims have explicit fairness assumptions
  and checked temporal properties; kept mutants must fail by the
  expected invariant, not by parse errors.
- Rust application source/tests/examples move into `crates/rootle`;
  root Cargo.toml becomes a virtual workspace. Pure provider vocabulary
  stays below implementations; app composition stays in the app crate.
  No obsolete compatibility re-exports or orphaned modules. Split large
  multi-concern production and test files by responsibility, not by line
  count alone. Domain identities and coordinates are named types; local
  arithmetic and named layout constants need no wrapper ceremony.
- Docker build/test/e2e/model, CI path filters, release version lookup,
  dependency-ordered publishing (including rootle-diff), macOS checks,
  site version/changelog sourcing, and provider conformance all follow
  the new package layout. Release verifies the actual packaged sources
  and binaries, not only checkout builds.

## Ownership while implementation is concurrent

- List owner: shared engine and the five existing list surfaces, their
  state/key/render siblings, plus Miller-pane reuse where appropriate.
  Expose typed selection/display offsets and one filter session API.
- Keymap owner: keymap registry, root named-key dispatch, preview motion
  and global-search input. No edits to the list owner's files. List
  navigation tables live with the list engine and are consumed rather
  than mirrored.
- Protocol owner: specs, TLC configs/gate, stdio transport and its
  behavioral tests, protocol model documentation. Main owns new commit
  provider fields and backend commit retrieval.
- Main: commit viewer correctness, provider commit contract, packaging,
  module/crate relocation, site, final verification and release. Move
  application paths only after concurrent source writers finish.
- All agents skip formatters, linters and build/test gates. Main runs
  validation after integration; agents report exact files and risks.

## Verification and landing

Exercise the real headless and PTY flows before final docker gates;
retain regression tests only for observable failure cases. Review the
rendered website before pushing its layout-dependent changes. Update
plans, contributor contracts, public docs and changelog to measured
outcomes. Push a PR with terminal frames and command evidence, wait
for CI, merge using the repository's owner policy, tag the matching
workspace version, and verify publication, tarballs, tap and site.
Do not publish claims of unbounded soundness/completeness or mark a
release shipped before its workflow completes.

## Local evidence (2026-09-07)

- Rust: fmt and clippy `-D warnings`; 282 tests, including 58 frame
  scenarios. Help-count clipping reproduced as `1 != 18`, then fixed
  by measured display widths with the regression retained.
- e2e: 51 cases on the host and in Docker; forge-conformance: 47 cases.
- Protocol model: corrected base safety/temporal checks and all four
  expected mutant failures passed through the Docker model gate.
- Real PTY: history → detail → delta → Esc ladder; commit key help,
  filter mode, resize, popup residue check and terminal restoration.
- Additional real-app smoke: full-message scrolling, filtered file
  indices, `]` prefix followed by `f`, reverse stepping, Unicode/tab
  paths, CRLF/no-final-newline and binary notices in Mocha and Latte.
- `cargo package --workspace --allow-dirty --locked` verified all six
  extracted crate archives. Each now includes the shared MIT license.
  The yanked transitive `chacha20` 0.10.1 was updated to 0.10.2 only.
- Site build reads `workspace.package.version` with TOML parsing;
  commit settings, protocol summary, roadmap and changelog are built
  from the real source tree; the published pages were checked after deployment.

## Published evidence

- [Implementation CI](https://github.com/rootledev/rootle/actions/runs/34070495900):
  `test`, `e2e-macos` and `forge-conformance` all passed before the owner merge.
- [Release workflow](https://github.com/rootledev/rootle/actions/runs/34071055219)
  passed all four build/verification jobs and publication.
  [v0.10.0](https://github.com/rootledev/rootle/releases/tag/v0.10.0)
  tags commit `8c0db3b0318177060003a313e1d3f117b4edf03d`.
- Downloaded all four tarballs: every SHA-256 sidecar matched and every
  GitHub/Sigstore attestation verified against `rootledev/rootle`.
  The Linux x86_64 executable is stripped and static-PIE linked.
- A fresh `cargo install rootle --version 0.10.0 --locked --registry
  crates-io` downloaded all six published packages and built the app.
  Both this installation and the downloaded Linux binary passed the
  real Git → stdio provider → history → commit detail → delta smoke.
- [Homebrew checks](https://github.com/rootledev/homebrew-tap/actions/runs/34071417726)
  passed on Ubuntu and macOS after the automatic formula/cask update.
- [Site deployment](https://github.com/rootledev/rootledev.github.io/actions/runs/34071854231)
  passed. The live [roadmap](https://rootle.dev/docs/roadmap.html),
  [commit documentation](https://rootle.dev/docs/settings.html#commit-inspection)
  and [changelog](https://rootle.dev/changelog/) show the released work
  and preserve the deferred scope.
