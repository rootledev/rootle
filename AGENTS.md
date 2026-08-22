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
| `e2e/` | uv+pytest PTY harness driving the real binary |
| `tests/render.rs` | frame-level snapshots on ratatui's TestBackend |
| `demos/` | demo tape + fixture (`demo_setup.sh`), vendored VHS fonts |
| `skills/` | public skill: provider scaffolding + conformance gate |
| `.agents/skills/` | maintainer skills: component scaffolding, TUI debugging, demo capture, PR authoring |
| `plans/` | numbered release plans — milestone status flips in the same PR as the work |

## Contracts to read before changing behavior

- `doc/house-style.md` — component contract (actions unidirectional,
  modeline, keymap tables are the single source of truth, sanitize at
  the boundary, `/` filter on every list, scrollbar rules).
- `doc/provider-protocol.md` — the stdio wire format (v1.1: reader
  tolerance, `$/cancelRequest`, `located`, `data.kind` errors).
- `doc/development.md` — architecture + e2e harness details.
- `.agents/skills/rootle-pr/` — PR template + evidence contract
  (frames/screenshots, green matrix including the docker e2e gate).

## Workflow

- `main` is protected: PRs only, `test` check required. As the repo
  owner you merge with the admin override — bot PRs (demo artifacts)
  never get CI on themselves, that's expected.
- The `demo` workflow re-renders `doc/demo.gif` + screenshots when
  `src/`, `demos/`, or `e2e/` change and opens a `demo/artifacts` PR —
  merge those to keep docs current.
- The site (rootle.dev) builds from `website/` +
  `doc/*.md` via `website/build.py` in the `pages` workflow — edit docs
  in `doc/`, never `public/` (build output, gitignored).
- Releases: tag `vX.Y.Z` matching `Cargo.toml`'s version, push the tag;
  the release workflow verifies the binary, publishes to crates.io,
  then cuts the GitHub release.
