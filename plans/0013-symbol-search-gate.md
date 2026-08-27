# 0013 — Symbol search gate: `search/symbols` + Tree-sitter evaluation

Status: **gate passed (2026-08-27)** — the Tree-sitter spike says
"every forge": in-process symbol extraction over the blob cache is
cheap enough to be the default; provider-declared indexes stay
preferred when present. UI/wire implementation not started.

Spike numbers (2026-08-27, Ryzen 9950X3D, release build, corpus 908
files / 7.7 MiB — 251 real helix .rs, 300 real .py, 117 real .ts,
240 synthetic .go; symbols = {name, kind, path, line} via each
grammar's tags query; note: query compiled per file, so these are
conservative):

- 16 840 symbols in 2.84 s → **320 files/s, 2.7 MB/s** single-threaded
- per file: rust 5.2 ms, python 3.1 ms, ts 2.3 ms, go 1.3 ms
- RSS delta for the whole corpus: **~13 MiB**
- a 1k-file repo cold-parses in ~3 s (once — the sha-keyed symbol
  cache makes repeats free, and files parallelize trivially if cold
  starts ever matter)

Decision: **every-forge local extraction** (spike-passes branch).
Providers declaring `symbols: true` (GitHub `symbol:`, GitLab
advanced search) are preferred; the local path parses cached blobs
with tree-sitter and labels results `parsed <short-sha>`.

## Problem

GitHub's web code search offers `symbol:` (Tree-sitter-parsed symbol
**definitions**) and index-based code navigation (jump-to-def,
find-references). rootle has neither — this is the largest remaining
gap in the head-to-head audit. This plan adds symbol search behind a
capability gate and evaluates whether rootle can offer it on **every
forge** (not just indexed ones) by parsing blobs in-process.

The framing: LSP-shaped requests, index-backed truth, honestly
labeled. rootle must say "index-based, may be stale" the way the
`stale` chip already says "approximate" — never pretend compiler
exactness (that is the LSP's job, not a forge's).

## Gate decision first (one spike)

**Can rootle offer symbol search on every forge?** The blob cache
holds full files; Tree-sitter grammars are pure data + a parser lib;
the search view already knows repos, trees, and shas. A working
spike:

- Vendor a small grammar set (rust, python, js/ts, go) via the
  `tree-sitter` crate + grammars crates.
- Given a repo's cached tree + blobs, extract
  `{name, kind, path, line}` symbol tables on demand (cache them next
  to blobs, keyed by sha — content-id correctness is free).
- Measure: symbols/sec, memory on a 1k-file repo, startup cost.

Outcomes:
- **Spike passes** → symbol search is a rootle-side feature over the
  blob cache; every forge (github, gitlab, bitbucket, fs, GHE) gets
  it by serving blobs. Providers can still declare an index-backed
  alternative when they have one (GitHub's native `symbol:`, GitLab
  advanced search) — prefer provider-declared, fall back to local.
- **Spike fails** (cost too high) → capability `symbols: true` and
  only providers with real indexes (github, gitlab) declare it.

Either way the wire shape below lands; the spike only decides whether
the default answer is "every forge" or "two forges".

## Wire shape (v1.4 candidate, additive)

Capability: `symbols` (bool, default false).

```
→ {"jsonrpc":"2.0","id":9,"method":"search/symbols","params":{
     "q":"render", "repo":null, "org":null}}
← {"jsonrpc":"2.0","id":9,"result":{"items":[
     {"repo":"o/r","path":"src/ui.rs","sha":"…","branch":"main",
      "name":"render","kind":"function","line":42,"signature":"fn render(view: &View) -> Frame"},
     …], "truncated": false}}
```

- `kind` is a closed enum mirroring LSP SymbolKind's common cases:
  `function | method | struct | class | interface | enum | variable |
  constant | module`.
- Progressive streaming rides the v1.3 `$/partial` mechanism — same
  notification, `search/symbols`-shaped items.
- `signature` optional (pretty chips in the result row); `line`
  required (we own line anchors since v1.3).
- Adapters with native symbol search (GitHub code search's `symbol:`,
  GitLab advanced search) translate to their grammar; the spike's
  local path ignores the method entirely (rootle computes from the
  cache — the provider is never asked).

## UI

- `␣ s` opens the search view in a third kind: `symbols` beside
  `grep` and `file find` (SearchKind gains it; the kind chip exists
  already). Query is the bare symbol name; scope radio works as-is.
- Result rows: `kind` chip (colored like badges) + name + signature
  (dim) + path:line; `Enter` opens the full-file preview at the
  definition line (the 0012 M2 pane).
- Kind colors come from `Theme` semantic roles (add `badge_fn`,
  `badge_type`, `badge_var` to the palette schema, Mocha defaults,
  all 11 palettes — same pattern as the `forge` role).

## Honesty labels

- Results from a provider index carry the existing freshness
  machinery (`located`, `index.as_of`).
- Results from the local spike path are parse-exact for the cached
  commit and must say so: `parsed <short-sha>` chip instead of an
  index date.

## Verification

- Wire: wiremock fixture for `search/symbols` + streaming; capability
  false → honest error kind `provider`.
- Spike gate: benchmark results recorded in this plan before any UI
  work begins; the chosen branch (every-forge vs two-forge) is
  documented here with the numbers.
- Render tests: kind chips, signature fitting, open-at-definition.
- e2e: fs provider extended to serve `search/symbols` from its files
  (it parses with the same grammar set — the reference adapter again
  demonstrates the method).

## Explicitly out

`symbol/references` (index-based find-refs): no backend's REST offers
it truthfully enough (GitHub's web UI has it, REST does not; GitLab's
advanced search covers identifiers, not semantics). Revisit when two
backends can answer it — a one-provider capability teaches the
protocol nothing.
