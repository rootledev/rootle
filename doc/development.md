# Development guide

How rootle is built, tested, and shipped. For using the app see the
[README](../README.md) and [settings.md](settings.md); for the component
contract see [house-style.md](house-style.md); for backend integration
see [provider-protocol.md](provider-protocol.md).

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
  stdio.rs     external providers: lifecycle (spawn, handshake,
               respawn-with-backoff)
  stdio/       transport.rs (child process, reader thread, reply
               routing), wire.rs (Provider methods → round trips)
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

## Testing tiers

Three tiers, cheapest-first — a new behavioral test belongs in the
cheapest tier that exercises it (plans/0023):

1. **Frame tests** (`tests/render.rs`, in-crate `#[cfg(test)]`) —
   `TestBackend` renders, offline state injected via `App::with` +
   `handle_action`. Deterministic; no workers, no subprocesses.
2. **Headless scripts** (`rootle --headless SCRIPT`, driver in
   `src/headless.rs`, suite in `e2e/test_headless.py`) — the real
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
- The suites run **offline**: `examples/providers/fs_provider.py`
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
`src/`, `demos/`, or `e2e/` it rebuilds, re-renders, and commits
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
