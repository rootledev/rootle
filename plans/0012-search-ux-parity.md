# 0012 — Search UX parity: grammar depth, full-file preview, facets

Status: **M1 + M2 done** (2026-08-27); M3 (facets) not started

## Problem

The gap analysis against GitHub's web code search (head-to-head audit,
2026-08) put delivery, previews, and honesty at or ahead — but the
query grammar is thin and the search view offers only folded regions
(≤5 lines × 2), never the whole file. Daily use should feel
head-to-head before symbol search lands (0013).

This plan is three independent milestones; any can ship alone.

## M1 — Query grammar: `language:`, quoted literals, `NOT`

The qualifiers are **provider-translatable** — no wire change:

- `language:rust` → each backend's language equivalent:
  - GitHub REST: native `language:` qualifier (pass through).
  - GitLab blob search: `lang:` filter.
  - Bitbucket/file-find: client-side post-filter on the tree extension
    (linguist-style ext→lang map, small static table in the adapter or
    in rootle for the fs fallback).
- Quoted literals (`"exact phrase"`) → single needle, not split terms
  (all backends accept multiword needles).
- `NOT` / `-` prefix on a term or qualifier → negation: GitHub
  native; everyone else: post-filter (subtract before display).

Rootle-side surfaces:
- `global_search/backend.rs::code_query` — today it builds
  `q` as `terms + scope + extension:` verbatim. It becomes a real
  grammar: tokenize (quotes, qualifiers, negation), then emit per
  provider capability (`Capabilities` gains nothing — grammar
  degrades gracefully: a backend that can't express a token drops it
  and the UI marks the query as partially filtered).
- Search view title shows the effective query vs the raw query when
  anything was dropped (honesty, like `located: false`).

Wire note: backends receive the final `q` unchanged — adapters
translate what they can. `fs_provider.py` gains `language:` +
negation post-filters so the reference adapter stays the worked
example.

## M2 — Full-file preview in the search view

Today: a grep hit shows ≤2 folded regions; `J/K` scrolls regions. The
ask: open the hit's **whole file**, scrolled to the match, in the
search view (no new view, no popup).

- Everything needed exists: blob fetch is sha-cached and lazy-located
  (`plans/0006` machinery), the `Preview` component renders numbered
  full files with the sign-column triangle, and the hit already
  carries repo/sha/branch/line.
- UI shape: `Enter` on a hit expands the results area into a file
  pane (same `Preview` component, re-used, not copied) positioned at
  `hit.line`; `Esc`/`h` returns to the results list. Keymap table rows
  in `src/keymap.rs` — hints derive automatically.
- The lazy-locate debounce (200ms) stays the cost model: expanding
  uses the already-fetched context blob; a second visit is free.

## M3 — Facets from the stream

Filter chips computed **from hits we already hold** — zero backend
cost:

- Beside/under the field row: per-repo chips and per-language chips
  (extension-derived) with hit counts; selecting one applies a local
  filter over the accumulated set (same path as the existing `/`
  filter — a facet is a committed filter with a visible source).
- Chips appear as results stream in and update as batches land; a
  cleared chip restores the full accumulated set.
- Extension mapping: reuse the M1 ext→lang table so chips read
  `rust` not `rs`.

## Sequencing

M2 first (biggest daily-feel win, zero wire risk), then M1, then M3.
They compose: M3's chips filter the set M2 previews; M1's effective
query shows above both.

## Verification

- Render tests: quoted/negated grammar → effective query shown; facet
  chip filters and restores; hit expand → full file with cursor at
  the anchor line, Esc restores results without residue.
- e2e: the fs provider drives all three (it must gain `language:` +
  negation first — it's the reference adapter).
- Docker gates (`fmt + clippy -D warnings + cargo test`, PTY suite)
  green; demo tape gets the expand flow only if it reads well on
  video.

## Out of scope

`symbol:` and navigation — those are 0013 with their own capability
gate. Sorting, saved searches — not worth the surface (GitHub doesn't
sort either).
