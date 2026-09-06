# 0025 — Typed identity: no naked strings, no bare counters

Status: **planned (2026-09-06)** — executes after 0024 (types land in
the new crates).

## Problem

Every identity in rootle is a naked `String`, and every staleness
guard a naked `u64`:

- repo = `"owner/name"` formatted/parsed at call sites
  (`App::last_commits: HashMap<(String, String, String), LogEntry>`);
- `sha: String` keys blob caches, pending/failed sets;
- `current_ref: Option<String>`, `at_commit: Option<(String, String)>`
  (browser.rs:65–70);
- VISUAL marks keyed by the string convention `"<pane title>/<entry
  name>"` (browser.rs:56–58) — any title edit silently orphans marks;
- `search_gen` / `view_gen` / `context_debounce_gen` raw `u64`s
  compared by `!=` at three landing sites;
- Action payloads mix `owner`/`name` split fields and joined strings
  for the same repo identity.

The bug classes this breeds are exactly the ones strop's
`strop-core/src/id.rs` kills: sha-for-repo mixups, a ref where a sha
was meant, generation compared against the wrong counter. None are
live bugs today; all are one refactor away.

## Decisions

1. **Newtypes in `rootle-provider`** (the seam where the vocabulary
   crosses): `RepoId`, `Sha`, `GitRef` — opaque wrappers over `String`
   (repos stay opaque `"group/project"` per the provider contract),
   `#[serde(transparent)]`, `Display`, cheap `Clone`. The `Provider`
   trait adopts them in one cutover (two impls + workers — contained),
   wire stays stringly.
2. **`Generation` newtype** wrapping the counters, with
   `Generation::next()` / `is_current(&self)` so landing sites read as
   a guard, not an inequality.
3. **`MarkKey { pane, entry }`** struct replaces the string-convention
   marks key.
4. **Action hygiene**: new/reworked variants carry typed identity
   (`RepoId`, `Sha`), no joined-then-split repo strings. The 85-variant
   enum stays one sectioned file (keymap-like single source).
5. **House style grows the type-discipline section**: identity
   crosses a boundary only as its newtype; strop's file ceiling
   adopted (~400 lines healthy, ~800 split — the established
   settings_popup/global_search pattern, now written down).
6. **Migration strategy is strop's, verbatim**: name the domains,
   tighten per call-site cluster as waves touch them. No big-bang
   rewrite of every string site in one PR.

## Not doing (recorded pushback)

- **No generational `Id<K>`/`Arena`**. Strop needs it because editors
  have a dynamic entity graph (documents/panes recycled by the arena).
  Rootle's entities are fixed-shape (one browser, fixed pane kinds,
  overlay slots) — typed names + `Generation` counters cover the same
  failure classes without a memory-management abstraction. If rootle
  ever grows dynamic panes, this decision is the one to revisit.
- **No `coordinate!`-style usize newtypes** for cursor/scroll yet:
  rootle has one coordinate domain (line index inside a component)
  vs strop's four; the payoff isn't there. Revisit with 0028's diff
  gutters if dual line-number domains (old/new side) leak into app
  state.

## Verification

Compile-driven (types catch the bugs), full render suite green,
headless `state` JSON unchanged (snapshot serializes through
`Display`).
