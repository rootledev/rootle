# AGENTS.md — working agreements for rootle

A modal ratatui TUI that browses remote source-control systems without
cloning: miller columns (orgs → repos → tree), syntax-highlighted
previews, Zed-style find/grep, clone wizard. Backends sit behind one
`trait Provider` — GitHub in-tree, anything else as an stdio child
speaking NDJSON-RPC (`doc/provider-protocol.md`).

## Build & test — docker first

Run the gate in containers, not on the host. The host has no Rust
toolchain contract; the image does — and `cargo build` on the host
leaves a `target/` tree that fights the container's own cache.

```
docker compose run --build --rm test      # fmt + clippy -D warnings + cargo test
docker compose run --build --rm e2e       # PTY e2e suite (pyte screen reconstruction)
docker compose run --build --rm release   # static musl binary → ./dist/
```

`--build` after every source change or the image runs stale code
(e2e reuses the test stage's compiled cache — the failure mode is
silent staleness). Host `cargo test` is fine for fast iteration while
developing; always finish with the two compose gates. `./dist/` and
`target/` may contain root-owned files (container mounts) — delete via
docker or `sudo`.

## Where things are

| Path | Contents |
|---|---|
| `src/app/` | event loop glue: dispatch, worker spawns (`workers.rs`) |
| `src/components/` | every UI piece behind the Component contract |
| `src/provider/` | the seam: `mod.rs` (trait), `stdio.rs` (external), `github.rs` |
| `src/github/` | GitHub-only internals (REST client, wire models, disk cache) |
| `src/headless.rs` | `--headless` scripted driver: keys in, frames/state JSON out (no PTY) |
| `e2e/` | uv+pytest harness driving the real binary — headless scripts + PTY suite |
| `tests/render.rs` | frame-level snapshots on ratatui's TestBackend |
| `demos/` | demo tape + fixture (`demo_setup.sh`), vendored VHS fonts |
| `skills/` | public skill: provider scaffolding (gate: forge-conformance) |
| `.agents/skills/` | maintainer skills: component scaffolding, TUI debugging, demo capture, PR authoring |
| `plans/` | numbered release plans — milestone status flips in the same PR as the work |

## Contracts to read before changing behavior

- `doc/house-style.md` — component contract (actions unidirectional,
  modeline, keymap tables are the single source of truth, sanitize at
  the boundary, `/` filter on every list, scrollbar rules).
- `doc/provider-protocol.md` — the stdio wire format (v1.3: `$/partial`
  progressive results + inactivity deadlines, reader tolerance,
  `$/cancelRequest`, `located`, `data.kind` errors).
- `doc/development.md` — architecture, testing tiers, e2e harness details.
- `.agents/skills/rootle-pr/` — PR template + evidence contract
  (frames/screenshots, green matrix including the docker e2e gate).

## Workflow

- `main` is protected: PRs only, `test` check required. As the repo
  owner you merge with the admin override.
- The `demo` workflow re-renders the demo GIFs (one per palette) when
  `src/`, `demos/`, or `e2e/` change and commits them to the site
  repo's `img/` (needs the `SITE_REPO_TOKEN` secret) — the site
  redeploys itself on push. Renders stage in gitignored `demos/out/`.
- The site (rootle.dev) is its own repo:
  `rootledev/rootledev.github.io` — landing (`website/`), user docs
  (`doc/`: settings, themes), demo GIFs (`img/`). Editing that repo is
  all it takes to update the site; this repo keeps only contributor
  contracts in `doc/` (house-style, provider-protocol, development).
  User-facing docs get edited on the site, never mirrored back here.
- Releases: tag `vX.Y.Z` matching `Cargo.toml`'s version, push the tag;
  the release workflow verifies the binary, publishes to crates.io,
  cuts the GitHub release, bumps the formula in
  `rootledev/homebrew-tap` (needs the `HOMEBREW_TAP_TOKEN` secret),
  and pings the site repo to redeploy (fresh version stamp; uses
  `SITE_REPO_TOKEN`).
