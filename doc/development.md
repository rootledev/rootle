# Development guide

How rootle is built, tested, and shipped. For using the app see the
[README](../README.md) and [settings](https://rootle.dev/docs/settings.html); for the component
contract see [house-style.md](house-style.md); for backend integration
see [provider-protocol.md](provider-protocol.md).

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
```

`crates/rootle/src/provider/` is the composition root, not the provider
trait. It selects implementations and owns config-writing lifecycle glue.
The GitHub client's siblings separate transport/auth, trees, blobs, search,
refs, history and blame; no resource implementation depends on the TUI.

Data flow rules (violations get caught in review):

- **Components never call each other.** They emit `Action`s; `App`
  routes them; cross-component effects flow back through `update`.
- **Worker results return as `AppEvent`s** with domain-tagged request
  generations. Stale results are rejected by identity; advisory
  cancellation never substitutes for the freshness check.
- **Provider calls run off the UI thread.**
- **Styling happens at the boundary** — raw search hits and blobs are
  highlighted once in `App` (`finish_hits`, blob path), not per frame.
- **Everything network/file-sourced passes `sanitize.rs`** before it
  reaches render state.
- **Shared list mechanics** live in `components/list_view`: item
  selection and display-row scrolling are separate types; filter
  sessions are incremental and reversible. Refs, help, settings,
  clone lists, history and commit files consume the same engine.
- **Bindings own both dispatch and hints.** Typed command tables feed
  input handlers; multi-key sequences retain prefix state, not aliases.
- **Commit patches are parsed on update**, not during draw.
  `rootle-diff` owns checked hunks and side-correct emphasis; the
  component owns file/message/delta navigation and palette-based rendering.

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

All six packages share `workspace.package.version`. Release publishing
orders provider/diff before their consumers and the app last. The model
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
   `<up|down|left|right>` tokens), `settle` (drain workers to
   quiescence), `wait <ms>`, `frame` (cell-grid dump), `state` (JSON:
   mode/overlays/context/status/yanks/editor_jobs). `-` reads the
   script from stdin; viewport via `ROOTLE_HEADLESS_COLS/ROWS`
   (default 100×30). This is also the review/stress surface for
   agents: pipe a script in, read frames out — no PTY, no timing
   heuristics, no ANSI.
   State is real: reuse a HOME across runs and the second run starts
   warm (recents, no launch popup) — scripts that need the launch
   popup must use a fresh HOME (the round-1 breaker hit this).
3. **PTY smoke** (`e2e/test_pty.py` — five tests, nothing else) — only
   for what a terminal proves: alternate-screen enter/leave, exit
   code, merged-ESC byte parsing, $EDITOR suspend/resume, resize
   redraw, TERM=dumb.

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
