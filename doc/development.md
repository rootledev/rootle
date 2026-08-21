# Development guide

How rootle is built, tested, and shipped. For using the app see
[getting-started.md](getting-started.md); for the component contract
see [house-style.md](house-style.md); for backend integration see
[provider-protocol.md](provider-protocol.md).

## Architecture

One process, one event loop, blocking providers on worker threads.
The shape is unchanged since v0.1 (plans/0001 §6); the provider seam
(plans/0005) is the one big cut since.

```
main.rs        terminal lifecycle, event loop, editor suspend/resume,
               clipboard write (queued by App, drained outside draw)
app/
  mod.rs       root: mode stack, action dispatch, overlays, rendering
  workers.rs   provider calls → worker threads → AppEvents (mpsc)
provider/
  mod.rs       trait Provider + shared types + build()/offline()
  github.rs    GitHub impl (auth chain: ROOTLE_TOKEN → GITHUB_TOKEN →
               `gh auth token` → anonymous)
  stdio.rs     external providers: NDJSON-RPC child over stdio
github/        GitHub-only internals: REST client, wire models, and
               the content-addressed disk cache (provider-internal —
               the TUI never touches it)
components/    the UI tree (see house-style.md)
```

Data flow rules (violations get caught in review):

- **Components never call each other.** They emit `Action`s; `App`
  routes them; cross-component effects flow back through `update`.
- **Worker results return as `AppEvent`s** with a generation counter;
  stale responses are dropped by generation, never cancelled.
- **Provider calls run off the UI thread.** Worst-case latency is one
  250ms poll tick.
- **Styling happens at the boundary** — raw search hits and blobs are
  highlighted once in `App` (`finish_hits`, blob path), not per frame.
- **Everything network/file-sourced passes `sanitize.rs`** before it
  reaches render state.

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
cargo fmt && cargo clippy --all-targets && cargo test   # host, fast loop
cd e2e && uv run pytest                                  # PTY e2e, host
docker compose run --build --rm test                     # the gate
docker compose run --build --rm e2e                      # e2e in-container
docker compose run --build --rm release                  # static musl binary → ./dist/
```

CI (`.github/workflows/ci.yml`) runs the gate and the e2e service on
every push; tags build the release artifact via `release.yml`.

Commits: small, theme-grouped, `feat:/fix:/test:/docs:` prefixes.
Plans live in `plans/` per release; flip milestone status in the same
PR that ships the work. PRs follow the
[rootle-pr](../.agents/skills/rootle-pr/SKILL.md) skill (evidence required).

## The e2e harness (`e2e/`)

A uv-managed pytest suite that drives the **real binary** on a PTY
and reconstructs the screen with pyte — the live complement to
`TestBackend` frame tests in `tests/render.rs`.

- `tui.py` — the driver. Hermetic per test: HOME/XDG point at a temp
  dir (`VISUAL=true` makes editor-open a no-op). Window size is set
  on the PTY **before** spawn (0×0 PTY = blank screen = looks hung).
  Output settling is quiescence-based (pump until the app stops
  repainting), which is both faster and more robust than fixed sleeps.
  `expect()`/`expect_gone()` poll with the screen dumped on timeout.
  Also records asciinema v2 casts (debugging only — see
  [rootle-demo-capture](../.agents/skills/rootle-demo-capture/SKILL.md)
  for why casts must not be rendered to GIF).
- `conftest.py` — fixtures: `tui` (plain app), `provider_tui` (fs
  stdio provider over a temp root), helpers like `open_fs_repo`.
- The suites run **offline**: `examples/providers/fs_provider.py`
  serves temp dirs as repos, so search → tree → preview → grep →
  clone all exercise the real stdio protocol with no network.
- Offline unit/frame tests inject `provider::offline()` and mock
  results through `App::with` + `handle_action` — workers never spawn.

Gotchas that have bitten (all covered by the suite):

- ESC bytes sent back-to-back merge into `Alt+<key>` in crossterm's
  parser — send ESC one call at a time.
- A stdio provider's child must die with rootle (`StdioProvider::drop`
  kills it); the lifecycle test enforces it.
- `docker compose run` needs `--build` after source changes or it
  runs a stale image.

## Demo + screenshots

`doc/demo.gif` renders from `demos/demo.tape` via the VHS docker image;
`doc/img/*.png` render from `demos/shots.py` (pyte screen → PNG). Both
carry gotcha lists in the
[rootle-demo-capture](../.agents/skills/rootle-demo-capture/SKILL.md)
skill; re-capture when any shown surface changes — or let the `demo`
workflow (`.github/workflows/demo.yml`) do it: on pushes touching
`src/`, `demos/`, or `e2e/` it rebuilds, re-renders, and commits
changed artifacts back to main (`[skip ci]`, idempotent).

## Skills (`.agents/skills/`)

| Skill | When |
|---|---|
| rootle-component | adding any UI component |
| rootle-tui-debug | verifying/debugging terminal behavior |
| rootle-demo-capture | demo GIF + doc screenshots |
| rootle-pr | authoring PRs (evidence contract) |

Public skill (`skills/rootle-provider`) scaffolds external providers with
a conformance-test gate.
