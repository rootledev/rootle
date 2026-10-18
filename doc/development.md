# Development guide

How rootle is built, tested, and shipped. For using the app see the
[README](../README.md) and [settings](https://rootle.dev/docs/settings.html); for the component
contract see [house-style.md](house-style.md); for backend integration
see [provider-protocol.md](provider-protocol.md).

## Supported platform and pilot authority

Only Linux/WSL x86_64 is actively supported. The release target and package
suffix are `x86_64-unknown-linux-musl`; source installs obey the same OS/CPU
policy. ARM, macOS and native Windows are not active targets. Portable
provider vocabulary, upstream lockfile entries and historical plans are
retained where they are not active platform implementations.

Actions still owns required checks and publication. The shared Buildbot
candidate is a **trusted-code, non-publishing pilot** on existing WSL/Docker,
not a replacement authority or an isolated public-fork worker. It receives
no registry, signing, tap or site-deployment credentials. Strong isolation
and signed owner promotion await the later storage migration.

### Candidate gates and exact artifact contract

The ordered machine-readable contract is [`.ci/pilot.json`](../.ci/pilot.json):
Rust gate → TLC/mutants → full e2e → pinned forge conformance → audit →
package → exact-package installed consumer. Run its argv vectors serially
from the clean committed candidate checkout with Docker Compose v2. The
controller supplies a CI-only HOME and unique Compose project per attempt;
this does not make sibling Docker build containers strongly isolated.

```sh
docker compose run --build --rm test
docker compose run --build --rm model
docker compose run --build --rm e2e
# forge-conformance command below
docker compose run --build --rm audit
docker compose run --build --rm -e VERSION=0.12.2 release
docker compose run --build --rm artifact
```

Docker compilation inherits `CARGO_BUILD_JOBS=4` inside RUN steps, not merely
from the worker environment. TLC retains `-workers 2` and `-Xmx2g`.
BuildKit's cached successful test/model layers may be reused: `--build` is
not a promise of fresh execution. Receipts must distinguish retained cached
gate evidence from newly executed runtime e2e/audit/consumer gates; do not
delete owner-wide caches to manufacture freshness.

### Immutable build inputs

Dockerfile pins the linux/amd64 Rust 1.98.0 / Alpine 3.24 image, Java 21 image
and Dockerfile frontend by observed manifest digest. The Rust image is the
amd64 child of the qualified `rust:alpine` index
`sha256:a10e64dd139b7387337c7fbe8aca31b959b57b2fd4c8ae20a02cf1d6ea424dce`.
[`docker-apk.lock`](../docker-apk.lock) records SHA256-verified complete APK
closures; installation disables network/index resolution after downloading
the pinned bytes. Python/uv/git and native compiler dependencies therefore
cannot silently move with an Alpine repository refresh. uv interpreter
downloads are disabled; `e2e/uv.lock` and `Cargo.lock` retain dependency hashes.
Rustfmt/clippy are bound to Rust 1.98.0; cargo-audit 0.22.2 uses a verified
crate digest and its bundled dependency lock. TLC 1.8.0's currently observed
release asset is pinned by checksum, rather than trusting its mutable URL.
These are input identities, not a claim that the candidate has passed gates.
CDN/registry/Rust/GitHub/crates/Python package retention and network access
remain prerequisites; missing or replaced pinned bytes fail closed.

The pilot rehearses the existing workspace version, without a version bump,
tag or publication. Use an empty candidate-owned `dist/`; never clean a user's
Docker objects or original checkout. The complete artifact inventory is
exactly:

```text
rootle-0.12.2-x86_64-unknown-linux-musl.tar.gz
rootle-0.12.2-x86_64-unknown-linux-musl.tar.gz.sha256
```

Substitute the workspace/tag version for future releases. The tarball contains
`rootle-<VERSION>-x86_64-unknown-linux-musl/rootle`. Release CI checks exact
filenames (not a magic file count), verifies the checksum, rejects ELF
`NEEDED` dependencies, checks `file` for static linkage, runs the real
headless popup open/close smoke, and checks `--version`. Preserve all these
obligations for the pilot; a successful archive command alone is not a gate.
The model gate retains all four named mutants, not only the positive TLC run.

Forge conformance is a separate required gate against the fs reference
provider. Its helper fetches a temporary checkout of `rootledev/forge-conformance`,
requires an immutable full SHA, and verifies HEAD before executing:

```sh
docker compose run --build --rm e2e \
  uv run --locked --no-sync python /app/.ci/forge_conformance.py \
  75c00748f6927e45ec23d42642ee96d96a787648
```

Use the pinned e2e image's Python, locked pytest dependencies and git:
missing git must not turn revision conformance into skipped coverage.
The existing `audit` Actions workflow remains required security evidence
(lockfile changes and weekly RustSec scan). The pilot's `audit` Compose
service uses pinned cargo-audit and records its JSON report in
`.ci-output/audit.json`; retain the advisory-db identity from that report
with the controller receipt. Advisory data intentionally refreshes at run
time: a stale advisory snapshot is not a fresh audit.

### Installer and artifact consumers

`install.sh` rejects unsupported OS/CPU pairs before version resolution,
network or filesystem mutation, requires the checksum sidecar, and installs
only the Linux x64 tarball. CLI startup checks support before diagnostic
files; self-update and provider-install APIs also guard their mutation
boundaries. Existing package-manager installs get their Linux upgrade
guidance rather than an in-place replacement.

The e2e suite includes unsupported-installer/no-mutation cases, missing/bad
checksum failures, and verified installation followed by real headless and
PTY fs-provider browsing. `ROOTLE_E2E_BINARY` accepts an absolute existing
binary path and bypasses compilation explicitly, so the same suite can
qualify the extracted release rather than only a debug build:

```sh
docker compose run --build --rm \
  -v /absolute/path/to/extracted-package:/candidate:ro \
  -e ROOTLE_E2E_BINARY=/candidate/rootle \
  e2e uv run --locked --no-sync pytest
```

The normal `artifact` gate invokes the same `.ci/qualify_release.py` used by
Actions: exact inventory/checksum → strict archive layout → static x86_64 ELF
and version/headless checks → original archive served on loopback →
`install.sh` into a disposable HOME → full pytest suite against that installed
binary, including real PTY browsing. It never republishes or rebuilds the
release binary. `--inventory-only` is the publisher's final pre-attestation
check; `--version` must match the workspace version. Self-update tests retain
their isolated replacement and provider-sweep ownership contracts.

Site and tap are separate repositories and must be qualified from isolated
checkouts. The site serves this repository's installer; land the Linux-only
installer before a later coordinated site deployment. The Linux x86_64
Homebrew source formula survives; the active macOS cask and its release bump
are retired. Historical releases, checksums and provenance stay unchanged.

## Architecture

One process, one event loop, blocking providers on worker threads. All Rust
source is in workspace crates; the root manifest only defines the workspace.

```
crates/rootle/    application: component tree, actions, worker integration,
                 headless driver, config, terminal lifecycle
crates/provider/ Provider trait, wire vocabulary, typed identities and paths
crates/stdio/    request routing, reader epochs, handshake/recovery and RPC wire
crates/github/   provider implementation, cache, resource-oriented HTTP client
crates/manager/  provider binary installation, receipts and CLI output
crates/diff/     checked unified-patch parsing and side-correct changed spans
crates/trace/    private bounded JSONL sessions, correlation, failure and completion
```

`crates/rootle/src/provider/` is the composition root, not the provider
trait. It selects implementations and owns config-writing lifecycle glue.
The GitHub client's siblings separate transport/auth, trees, blobs, search,
refs, history and blame; no resource implementation depends on the TUI.

Data flow rules (violations get caught in review):

- **Sibling components never call each other.** Parents own and compose
  children; cross-component effects flow through `Action`s and `App::handle_action`.
- **Worker results return as `AppEvent`s** with domain-tagged request
  generations. Stale results are rejected by identity; advisory
  cancellation never substitutes for the freshness check.
- **Provider calls run off the UI thread.**
- **Styling happens at the boundary** — raw search hits and blobs are
  highlighted once in `App` (`finish_hits`, blob path), not per frame.
- **Tree-sitter grammars are statically linked.** `highlight/registry.rs`
  detects filenames without local filesystem reads, `queries.rs` composes
  upstream queries and injections, `styles.rs` maps captures to palette
  roles, and `render.rs` streams events into owned terminal lines.
  Compiled queries are cached per language; parser state survives calls
  and theme changes. Alpine `build-base` compiles the grammar C/C++ sources;
  release gates reject ELF `NEEDED` dependencies, including static PIE.
- **Everything network/file-sourced passes `sanitize.rs`** before it
  reaches render state.
- **Shared list mechanics** live in `components/list_view`: item
  selection and display-row scrolling are separate types; filter
  sessions are incremental and reversible. Refs, help, settings,
  clone lists, history and commit files consume the same engine.
- **Bindings own both dispatch and hints.** Typed command tables feed
  input handlers; multi-key sequences retain prefix state, not aliases.
  Commit files, prose and diffs use distinct `CommitContext` tables; a
  transient diff find owns input until submission/cancellation and never
  falls through to the sidebar filter. Literal matching is shared with
  preview find and maps case-folded matches back to original UTF-8 bytes.
- **Commit patches are parsed and highlighted on update**, not during draw.
  `rootle-diff` owns checked hunks and side-correct emphasis; the app's
  commit component builds each hunk's old/new source fragments for the
  shared Tree-sitter highlighter. Missing context is not reconstructed.
  Preview prose, pane chrome, styled range overlays and list/row viewport
  behavior are shared rather than reimplemented inside the commit viewer.
- **Commit file hierarchy is display-only.** Paths are indexed once;
  directory headings are not selectable and never count in file stepping.
  Source-line permalinks use typed path/revision/line targets, with deleted
  lines anchored to the first parent and pre-rename path.
- **CLI command ownership is explicit.** `RootCommand` distinguishes
  application-only `self-update`, combined `update`, and nested `provider`
  commands. Execution validation rejects ignored UI arguments while global
  diagnostic flags remain valid before or after a command.
- **History requests carry full identity:** repository, optional revision,
  file/repository scope and a domain-tagged generation. Both successful
  and failed outcomes must match the current request.
- **Fresh profiles have no provider suggestions.** Only user recents
  populate the browser; repository search is the cold-start entry point.

Overlays are exclusive slots on `App` (`popup`, `search_view`, `help`,
`command_line`, `settings`, `wizard`); dispatch checks them topmost
first. The leader layer works over the search view by routing keys to
`keymap::leader` while `mode == Leader`.

## Development workflow

The gate (fmt + clippy -D warnings + cargo test) runs as a Docker
build stage — **run the docker gate, not just host commands**: the
container's clippy is newer than the host's and has caught lints the
host misses.

```
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
uv run --directory e2e pytest                            # headless + PTY, host
docker compose run --build --rm test                     # the gate
docker compose run --build --rm e2e                      # e2e in-container
docker compose run --build --rm model                    # bounded TLC + kept faults
docker compose run --build --rm -e VERSION=0.10.0 release # static musl tarball → ./dist/
```

CI (`.github/workflows/ci.yml`) runs the gate and the e2e service on
every push; tags build the release artifact via `release.yml`.

All seven packages share `workspace.package.version`. Release publishing
orders trace/provider/diff before their consumers and the app last. The model
checks ten safety invariants plus type correctness and two
fairness-qualified temporal properties; four kept faults must violate
their named invariant. The executable routing model and real-child tests
are a bridge to Rust, not a formal refinement proof.

Commits: small, theme-grouped, `feat:/fix:/test:/docs:` prefixes.
Plans live in `plans/` per release; flip milestone status in the same
PR that ships the work. PRs follow the
[rootle-pr](../.agents/skills/rootle-pr/SKILL.md) skill (evidence required).

## Testing tiers

Three tiers, cheapest-first — a new behavioral test belongs in the
cheapest tier that exercises it (plans/0023):

1. **Frame tests** (`crates/rootle/tests/render/`, in-crate `#[cfg(test)]`) —
   `TestBackend` renders, offline state injected via `App::with` +
   `handle_action`. Deterministic; no workers, no subprocesses.
2. **Headless scripts** (`rootle --headless SCRIPT`, driver in
   `crates/rootle/src/headless.rs`, suite in `e2e/test_headless.py`) — the real
   binary, real provider subprocesses, real input path
   (`App::handle_key`); only the terminal byte layer is skipped.
   Script language: `keys <text>` (`<esc> <cr> <bs> <tab> <space>`
   `<up|down|left|right>` tokens), `settle [ms]` (wait for all tracked
   workers and queued follow-ups; default 10000ms), `wait <ms>`,
   `frame` (cell-grid dump), `state` (JSON:
   mode/overlays/context/status plus request-scoped browser/search outcomes). `-` reads the
   script from stdin; viewport via `ROOTLE_HEADLESS_COLS/ROWS`
   (default 100×30). This is also the review/stress surface for
   agents: pipe a script in, read frames out — no PTY, no timing
   heuristics, no ANSI.
   Startup uses the same tracked wait with a 10-second bound. After
   navigation, put `settle` before `frame`/`state`; channel silence
   alone is not completion. A deadline or malformed settle timeout
   fails the run rather than sampling a half-loaded surface. Provider errors
   still complete their work: inspect the current request's phase/error.
   State is real: reuse a HOME across runs and the second run starts
   warm (recents, no launch popup) — scripts that need the launch
   popup must use a fresh HOME (the round-1 breaker hit this).
3. **PTY smoke** (`e2e/test_pty.py` — five tests, nothing else) — only
   for what a terminal proves: alternate-screen enter/leave, exit
   code, merged-ESC byte parsing, $EDITOR suspend/resume, resize
   redraw, TERM=dumb.

### Headless readiness contract

`state_schema_version: 1` is additive: existing root fields remain. For a direct
repository load, run `settle`, then inspect `browser.tree`: `phase` is
`idle | loading | ready | failed`; `request` carries the exact repository,
requested revision (`null` for default) and generation. A ready tree's
`entry_count` counts the accepted recursive provider entries, including directory
entries, independently of filtering or viewport size. Empty success is `ready`
with count `0`; loading/failure counts are `null`. `truncated`, `branch` and typed
`error` accompany the current outcome. `displayed` separately identifies retained
prior data and marks it stale during a failed/pending replacement.

`browser.pane` reports depth/path, immediate entry count and filtered-visible
count. `browser.owner_list` owns its separate outcome. `search` is null when
closed; otherwise it exposes phase, frozen submitted request, retained/visible
hit counts, known truncation and typed error (`kind`, bounded sanitized `message`,
`retry_after_s`). Edited input is not a newly submitted request. Failed streams
retain partial hits, not a successful-empty result. Capability declarations are
reported separately; repo-local file find can work from the tree without global
file-search support.

Check the requested repository/ref and phase/count/truncation, not a universally
fixed entry count, `status == null`, or `job_finished` alone. A finished worker
may produce a stale reply that the UI rejects. `state` does not wait by itself;
accepted readiness does not prove a frame was painted or no overlay obscures it.
Session diagnostics use the same outcome projection while redacting query/error
text in metadata-only capture.

## Diagnostic sessions

Build the current checkout before investigating: an older installed release
may not have the diagnostic switches. Logging is opt-in and never writes
records to stdout/stderr or the active TUI.

```sh
rootle --log
rootle --log-file issue.jsonl owner/repo
rootle --headless steps.txt --log-file issue-full.jsonl --log-content
rootle provider list --json --log-file provider-list.jsonl
```

`--log` / `--log=ALL` chooses a new file under the state directory's
`rootle/logs/`. `--log=PATH` and `--log-file PATH` select a native path;
explicit CLI selection overrides `ROOTLE_TRACE=PATH`. The environment
knob is retained, but now writes JSONL rather than legacy text appends.
Files are exclusively created with Unix mode 0600; automatic directories
are 0700 where newly created. **Use a new filename for every run.**
Do not point at an existing `mktemp` file: create a temporary directory
and choose a filename inside it. Startup refuses existing files/symlinks
and missing explicit parent directories before entering raw mode.

### What to read

- `session_start` identifies version, driver, content policy and limits.
  `session_end.outcome` describes the command; final `trace_end.complete`
  describes capture completeness. Missing/malformed terminal records mean
  incomplete, including fatal aborts or storage failures.
- Input/action/state records share the application handlers in both
  drivers. State includes input-owner modes, overlays, input character
  cursors, filter sessions, selected items, display-row offsets and request
  generations. No whole-action/config Debug dumps.
- Worker `operation_id` connects job start/outcome with synchronous backend
  traffic. Match RPC tx/rx using `fields.session` + `fields.id`; UI receipt,
  acceptance and `job_rejected` records retain generation/identity reasons.
- HTTP records identify safe endpoints, status, duration and body byte
  counts, not bodies or authorization. Cache records distinguish hits,
  misses, revalidation and corrupt-data fallback.
- `render` records the actual grid and requested hardware cursor, with
  dimensions, frame number, draw microseconds and a SHA-256 cell/style hash.
  Resize records distinguish terminal notifications from observed draw
  geometry. Full captures add per-row cell runs:
  `[repeat, glyph, foreground, background, underline, modifiers, diff_option]`.
  Colors are reset=0, basic=1–16, indexed=256+index, RGB=`0x01000000|rgb`;
  diff options are none=0, skip=1, always-update=2, forced-width=`0x10000|width`.
- Config, update, editor/clipboard and provider lifecycle records explain
  side effects and errors without logging command argument vectors.

```sh
jq -c 'select(.event == "job_rejected" or .event == "error" or .event == "panic")' issue.jsonl
jq -c 'select(.operation_id == 7)' issue.jsonl
jq -c 'select(.event == "rpc_message") | {seq, thread, fields}' issue.jsonl
jq -c 'select(.event == "render") | .fields | del(.cells)' issue.jsonl
```

### Privacy and failure

Metadata is **not anonymous**: repository/path/revision identities and timing
can still be sensitive. Input text, query/command/status text, file text,
visible glyphs and stderr require `--log-content`; that mode can contain
secrets typed or displayed in the app. Inspect before sharing. Neither mode
automatically records env values, argv vectors, authorization headers or
raw HTTP/RPC bodies.

The default hard bounds are 64 MiB, 100,000 events and 256 KiB per record,
with a bounded queue and reserved terminal marker. Overflow/caps/I/O failures
stop recording, report once in the status path and fail finalization after
terminal restoration. Finishing and panic flushing wait at most five
seconds; abrupt failure may leave no marker. A marker's success does not
mean a provider request succeeded. This is evidence for the existing
headless driving workflow, not a replay of external services or timing.

## The e2e harness (`e2e/`)

A uv-managed pytest suite. Nearly everything is the headless tier:
`e2e/headless.py` (`run_headless` / `fs_config` / `states` /
`frames`) pipes scripts to `rootle --headless -` and asserts on
frames/state JSON — deterministic, no pyte. The terminal boundary
itself is pinned by `e2e/test_pty.py`, which drives the real binary
on a PTY and can inspect the raw escape stream (`Tui.raw()`).

- `tui.py` — the PTY driver. Hermetic per test: HOME/XDG point at a
  temp dir (`VISUAL=true` makes editor-open a no-op). Window size is
  set on the PTY **before** spawn (0×0 PTY = blank screen = looks
  hung). Output settling is quiescence-based (pump until the app
  stops repainting), which is both faster and more robust than fixed
  sleeps. `expect()`/`expect_gone()` poll with the screen dumped on
  timeout. Also records asciinema v2 casts (debugging only — see
  [rootle-demo-capture](../.agents/skills/rootle-demo-capture/SKILL.md)
  for why casts must not be rendered to GIF).
- `conftest.py` — the session `binary` fixture, hermetic helpers
  (`dismiss_launch_popup`, `open_fs_repo`), and the fs/git fixtures
  (`make_fs_root`, `make_git_root`).
  The host build honors `CARGO_TARGET_DIR`; use a directory outside the
  checkout when keeping host artifacts separate from container builds.
- The suites run **offline**: `crates/stdio/examples/fs_provider.py`
  serves temp dirs as repos, so search → tree → preview → grep →
  clone all exercise the real stdio protocol with no network.
- Offline unit/frame tests inject `provider::offline()` and mock
  results through `App::with` + `handle_action` — workers never spawn.

Gotchas that have bitten (all covered by the suite):

- ESC bytes sent back-to-back merge into `Alt+<key>` in crossterm's
  parser — send ESC one call at a time. (Headless scripts feed
  discrete key events; `<esc><esc>` in one `keys` step is safe there.)
- A stdio provider's child must die with rootle (`StdioProvider::drop`
  kills it); the lifecycle test enforces it.
- `docker compose run` needs `--build` after source changes or it
  runs a stale image.

## Demo GIFs

`demo.gif` (canonical, Catppuccin Mocha) and the per-palette
`demo-<theme>.gif` variants (the website's palette picker swaps them
in) all render from `demos/demo.tape` via the VHS docker image — one
sed-parameterized run per embedded palette. Local renders land in
gitignored `demos/out/`; the published GIFs live in the site repo
(`rootledev/rootledev.github.io`, `img/`). Gotchas live in the
[rootle-demo-capture](../.agents/skills/rootle-demo-capture/SKILL.md)
skill; re-capture when any shown surface changes — or let the `demo`
workflow (`.github/workflows/demo.yml`) do it: on pushes touching
`crates/`, `demos/`, or `e2e/` it rebuilds, re-renders, and commits
the refreshed GIFs to the site repo's `img/` — the site redeploys on
push.

## Skills (`.agents/skills/`)

| Skill | When |
|---|---|
| rootle-component | adding any UI component |
| rootle-tui-debug | verifying/debugging terminal behavior |
| rootle-demo-capture | demo GIFs (per-palette) |
| rootle-pr | authoring PRs (evidence contract) |

Public skill (`skills/rootle-provider`) scaffolds external providers; the
canonical [forge-conformance](https://github.com/rootledev/forge-conformance)
suite is the integration gate (the `forge-conformance` CI job runs it
against `fs_provider.py`).
