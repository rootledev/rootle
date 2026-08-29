# 0021 — Architecture pass: modularity splits, keymap grammar, provider concurrency

Status: **accepted (2026-08-28)** — owner-blessed; M1 lands
with this flip. This is the plan, not the work.

## Problem

Sustained velocity left three debts, all confirmed by measurement:

1. **Modularity**: several files outgrew their seams —
   `app/mod.rs` 1929 lines, `browser.rs` 1183, `global_search.rs`
   1188, `preview.rs` 1082, `theme.rs` 954, `app/workers.rs` 572,
   `github/client.rs` 795 (the repo's own split conventions —
   `global_search/`, `manager/`, `app/workers.rs` — proved themselves
   and just need one more round).
2. **Keymap drift**: 0013 proposes `␣ s` for symbol search — which is
   repo search today. And popup contexts silently swallow verbs
   (`y` in the repo-search popup does nothing — that's what "browse
   yank feels broken" was; browse-level yank verified healthy at every
   level: org, repo, dir, file).
3. **Provider concurrency**: rootle-gitlab's `serve_stdio` is a
   strictly serial read-answer loop; protocol v1.3 allows id-tagged
   interleaving and rootle's transport already pipelines
   (`pending: HashMap<id, Sender>`) — a slow request (tree fetch,
   blame walk) stalls quick ones behind it.

## M1 — app/ decomposition (the monolith)

`app/mod.rs` keeps: the `App` struct, constructors, `render`. Move:

- `app/events.rs` — `handle_app_event` + event→state mapping (~600 ln).
- `app/actions/` — dispatch arms by domain, one file each, same
  `impl App` pattern as workers.rs: `actions/browse.rs` (navigation,
  filter, blobs, yank), `actions/search.rs` (view lifecycle, hit
  events, pane parity arms), `actions/lenses.rs` (refs, history,
  blame, blob_at, last-commit band), `actions/lifecycle.rs` (update
  notice, declaration/consent, settings, quit).
- The action enum stays one file (`action.rs`); no behavior changes
  allowed in M1 — pure move, zero logic edits.

## M2 — component splits at existing seams

- `browser.rs` → `browser/lenses.rs` (blame + history state machines,
  `BlameState` moves here), `browser/blobs.rs` (cache + blob
  lifecycle); `browser.rs` keeps columns/selection (tree.rs already
  split).
- `preview.rs` → `preview/lens.rs` (band + blame marks + the
  last-commit shaping), `preview/motion.rs` (vim motions + goto);
  `preview/find.rs` already split.
- `global_search.rs` → `global_search/pane.rs` (the expanded-pane
  methods: parity block, yank target, blame mirror); the rest already
  split.
- `theme.rs` → `theme/palettes/<name>.rs` data modules (11 palettes
  are ~800 lines of data); theme.rs keeps the schema + loaders.
- `app/workers.rs` → `workers/search.rs`, `workers/lenses.rs`,
  `workers/lifecycle.rs` (spawn_* by domain); mod.rs keeps the blob
  cap + styling helper.
- Budget rule for the repo: >800 lines → must justify or split;
  >600 → investigate.

## M3 — keymap grammar

- **Symbols**: `␣ s` stays repo search (settled grammar, launch popup).
  0013's symbol kind gets `␣ S` (capital = the heavier search) AND a
  kind radio in the search view (the scope-popup pattern) — both,
  because the radio is discoverable and the chord is fast.
- **Hygiene rule** (new conformance expectation): every context either
  honors a navigation verb or the strip shows why not — no silent
  dead keys. Concretely: the repo-search popup gains `y`/`enter`
  parity, and a keymap table cross-check test walks every context
  asserting each table row's key resolves to a non-Noop action.
- No new modes this round: the preview-submode pattern is the right
  shape for panes; a "git mode" is rejected (lenses ride the pane
  they're about — that's what made history/blame composable).
- Deferred with a name, not a shrug: history from the search pane —
  the lens renders browser-side; rendering it inside the search view
  is M3-adjacent but its own milestone when symbols land (they share
  the view).

## M4 — provider concurrency (sibling repos)

- rootle-gitlab: stdin loop stays the reader; each request spawns a
  worker (`respond_transcript` is already request-scoped); stdout
  behind a mutex (transcripts interleave by id — the v1.3 contract).
  Audit `Handler` for shared mutable state before spawning (disk
  caches are safe by construction; in-memory session state gets the
  audit). Same shape for rootle-bitbucket; the fs reference provider
  demonstrates it too (conformance stays the gate).
- Owner decision needed: `$/cancelRequest` becomes meaningful with
  concurrency (today advisory-ignored) — implement or stay silent
  while concurrent? Recommend: cancel honored for streaming searches
  only (the expensive ones).

## Decisions baked in

- M1–M2 are zero-behavior-change moves; each lands in its own PR
  with the full gate matrix (the moved code is the reviewable diff,
  not the argument).
- No renames of public symbols while moving (diffs stay moves).
- Keymap changes come with the `?` popup frame in the PR (the table
  is the source of truth — the popup derives from it).

## Verification

- M1/M2/M3: `cargo test` + e2e + docker gates green per PR; a
  mechanical diff review (moved lines equal old lines, modulo
  visibility markers); render snapshots unchanged.
- M3: keymap cross-check test (table row ↔ resolvable action, every
  context); the symbol-binding decision recorded in 0013's status.
- M4: rootle-gitlab's wiremock suite + a concurrency test (a slow
  mock request does not delay a fast one); conformance suite green in
  both sibling repos.
