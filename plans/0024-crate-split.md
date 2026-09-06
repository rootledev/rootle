# 0024 — The crate split

Status: **done (2026-09-06)** — commits 938a445+cdeed35-era scaffolding through 4a970eb; docker test gate green, e2e/model re-run on the combined tree. update.rs → selfupdate.rs; factory stayed app-side (src/provider/) — dependency reality over the plan's first sketch.
(0024–0028); this lands first so the rest lands in the new shape.

## Problem

Rootle is one 25k-line crate. Three concrete costs:

1. **Boundaries are discipline, not types.** Nothing stops
   `components/` reaching into `src/github/client.rs` internals or the
   transport; the provider seam (house-style "Provider seam") is a
   convention reviewed by eye.
2. **Every UI change rebuilds the world.** reqwest + syntect sit in the
   same unit as every component edit; the docker test gate pays it on
   every run.
3. **The self-contained pieces can't be tested in isolation.** The
   stdio transport, the GitHub client, and the manager are each
   meaningful units with their own test files (stdio/tests.rs is
   500 lines) but no compile-time identity.

strop ran this exact migration (7 crates, plans/0023/0026 there):
`strop-core` / `-grammar` / `-git` / `-lsp` / `-picker` / `-syntax` /
editor binary — and its AGENTS cites rootle's own
`settings_popup/{mod,render,sections}.rs` as the file-level pattern.
The crate split is the same rule one level up.

## Research findings

- Rootle's de-facto seams already exist as modules: `src/provider/`
  (trait + stdio + manager), `src/github/` (REST + cache), everything
  else app-side. `src/update.rs` is NOT dispatch — it is the CLI
  self-updater (confusingly named; renamed `selfupdate.rs` in M5).
- `src/provider/ui.rs` is std-only ANSI output for the manager CLI
  (uv/mise grammar) — it belongs with the manager, not the TUI.
- Action ↔ `components::global_search` type coupling stays inside the
  app crate (both sides remain in `rootle`); no boundary problem.
- tests/render.rs (2,460 ln, 57 tests) links the lib — it moves to
  `crates/rootle/tests/` unchanged.
- crates.io: `rootle` is published (v0.9.3); a workspace with path
  deps requires publishing every crate in topological order.

## Milestones

### M1 — workspace skeleton + `rootle-provider`

- Root `Cargo.toml` becomes `[workspace]` (members under `crates/`,
  resolver 2) with `workspace.package` (version/edition/rust-version/
  license) inherited everywhere.
- `crates/provider` (`rootle-provider`): the trait, wire types
  (`LogEntry`, `RepoRefs`, `RefInfo`, `BlameRange`, `TreeResult`,
  `CodeMatch`, `Capabilities`), `ProviderError`, `build()`,
  `BuildOutcome`, `offline()`. The 653-line `provider/mod.rs` splits by
  concern while moving: `lib.rs` (trait), `types.rs`, `builder.rs`.
  Deps: serde only.

### M2 — `rootle-stdio`

`crates/stdio`: `stdio.rs` surface + `stdio/{transport,process,
handshake,restart,wire}.rs` + the 500-line in-module test file. Deps:
`rootle-provider`, serde_json.

### M3 — `rootle-github`

`crates/github`: `github.rs` (Provider impl) + `github/{types,cache}.rs`
+ client — and the 795-line `client.rs` splits by resource while
moving (`client/{mod,refs,log,blame,tree,search}.rs`). Deps:
`rootle-provider`, reqwest, sha2, base64, flate2, tar.

### M4 — `rootle-manager`

`crates/manager`: `provider/manager.rs` + `manager/{install,release,
refs,store,bookkeeping}.rs` + `provider/ui.rs` (CLI ANSI). Deps:
`rootle-provider`, reqwest, serde.

### M5 — app crate + rename + docs

- `crates/rootle`: everything else (`app/`, `components/`, `action`,
  `event`, `keymap`, `mode`, `theme(+palettes)`, `config`, `state`,
  `headless`, `sanitize`, `paths`, `clipboard`, `highlight`, `editor`,
  `commands`, `cli`, `main`). `src/update.rs` → `selfupdate.rs` (name
  stops colliding with the Elm-update mental model). `lib.rs`
  re-exports stay the tests' interface.
- `tests/render.rs` → `crates/rootle/tests/render.rs`.
- Docs: AGENTS.md "Where things are" table, `doc/development.md`,
  Dockerfile/compose unchanged (context copy covers crates/).

### M6 — release pipeline publishes the set

release.yml gains ordered publishes: `rootle-provider` →
`rootle-stdio` → `rootle-github` → `rootle-manager` → `rootle`
(same workspace version). Crates.io name availability checked before
merge (`cargo search` / crates.io API); descriptions + categories per
crate. Version note: the split ships in v0.10.0.

## Verification

- `docker compose run --build --rm test` + `e2e` green after every
  milestone (each M is a compiling, green state — no big-bang).
- Zero behavior change: same binary CLI surface; headless frames of a
  reference script byte-identical pre/post (the driver exists for
  exactly this).

## Deliberately not doing

- No new `rootle-diff` crate yet — it arrives with 0028 where it gets
  content.
- theme/config/state stay app-side: single-consumer modules gain
  nothing from a crate boundary but churn.
- No trait signature changes (that is 0025) — pure moves plus the
  named file splits.
