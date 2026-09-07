# 0028 — The commit viewer

Status: **implemented and verified locally (2026-09-07)** — v1.6
`repo/commit` across GitHub/stdio/fs, checked `rootle-diff` hunks,
full-message/file/delta surfaces, real `]f`/`[f` sequences, palette-aware
diff roles and the Esc ladder. Real Git-backed headless and PTY flows
passed. The older-list migration deferral is closed by 0029.
The demo-story rework remains on the roadmap.

Uses the shared list engine (0026) and typed identity (0025).
Supersedes 0016's no-diff deferral while preserving its read-only boundary.

## Problem

The v1.5 revision layer is path-scoped browsing: a tig-shaped history
lens and a blame lens, both landing on `blob_at` (the file AT a
commit). There is no way to inspect a commit itself — its full
message, its stat, its changed files, a hunk of its diff. strop built
exactly this (plans/0010 there + the dive chain): typed diffs, real
navigation, decoration from typed data. rootle gets the reader's half
of it.

## Research findings (strop, adapted)

- **Typed diff model** (`strop-git/src/diff.rs`):
  `LineOrigin {Context, Addition, Deletion}`,
  `DiffLine { origin, old_lineno: Option<usize>, new_lineno,
  text: Vec<u8>, has_newline }` (absent side is `None`, never `0`),
  `Hunk` with derived `signs()`/`changed_region()`/`header()`,
  `FileDiff { path, hunks, added, deleted }`. Strop sources it from
  libgit2; **rootle sources it from the wire** — providers return
  unified-diff patch text (GitHub's `files[].patch` shape), so rootle
  needs its own patch→typed-hunks parser (strop's lesson in reverse:
  "parsing our own text was the original sin" applies to *rendering*;
  the wire boundary makes parsing someone else's text the honest job).
- **Dive chain** (`strop editor/dive.rs`): history row → commit
  detail (changed files) → file delta; `]f`/`[f` step files within
  the commit rewriting the delta in place; blame gutter Enter dives
  the line's commit; Esc/q unwind one surface at a time.
- **Renderer** (`strop render/diff.rs`, 711 ln): row addressing
  `enum DiffRow { Stats, HunkHeader, Line }` mirroring the content
  layout; gutter anatomy `[sign][old][new][content]` (absent side
  blank, never `0`; cursor marker `▸` in the origin's color — rootle's
  own triangle marker, cited in strop); two-tier delta emphasis —
  quiet full-row add/del backgrounds, loud intra-line span computed by
  pairing del/add runs and trimming common affixes; hunk headers and
  stats on a quiet structural band. Side-by-side, syntax-in-diff, and
  folding were pushed back with recorded reasons — inherited as prior
  decisions, listed in roadmap/Evaluating, not relitigated.
- rootle's theming contract overrides strop's hardcoded RGB: all diff
  colors become new `Semantic` roles with Catppuccin-Mocha defaults;
  palettes overlay.

## Milestones

### M1 — wire v1.6: `repo/commit`

`repo/commit {"repo","sha"}` → `{"sha","author","date","message",
"parents"?: [sha], "files": [{"path","status","additions",
"deletions","patch"?}]}` — `patch` is the file's unified hunks
(no file headers), absent for binary; `status` ∈ added/removed/
modified/renamed (renamed carries `previous_path`). Capability
`commit` (default false — same honest-chip family as refs/log/blame).
Trait method + types; GitHub impl (GET /commits/{sha}, patch strings
already in the response); fs_provider.py via `git show` + diff-tree;
stdio wire row; protocol method table, adapter gate and real-app coverage.

### M2 — `rootle-diff` crate

Pure, no deps beyond std: the unified-patch parser (hunk headers with
counts, `\ No newline at end of file` → `has_newline=false`, CRLF
bytes preserved), `FileDiff`/`Hunk`/`DiffLine`/`LineOrigin`, plus the
emphasis engine (del/add run pairing, common-affix trim, char-boundary
safe). Unit tests: root commits (all-old `None`), pure adds, paired
runs, shared affixes, no-final-newline, CRLF.

### M3 — surfaces + dive chain

Preview-region surfaces per the 0016 preview-submode doctrine:
- history lens row + `d` → **CommitDetail** (header band: sha ·
  author · date · +N −M; sanitized message body; changed-files list
  on ListView with `/` filter — house rule);
- Enter on a file row → **Diff surface** (M2 model through the M4
  renderer); `]f`/`[f` rewrite the delta in place ("label · i/n");
- blame lens Enter reaches the line's commit in history; `d` then
  uses the same commit-detail path rather than a second viewer;
- Esc/q unwinds one surface at a time; existing `Enter` on a history
  row (file at commit) unchanged.
- Keymaps registered in keymap.rs tables (0026 closure); snapshot()
  gains `surface` (kind, cursor, file position) for headless
  assertions; `Y` yanks the commit's provider-supplied web URL.

### M4 — renderer

`components/commit/{mod,render,dive}.rs` sibling split per
house-style. New `Semantic` roles: `diff_add_fg`, `diff_del_fg`,
`diff_add_bg`, `diff_del_bg`, `diff_add_strong`, `diff_del_strong`,
`diff_band` (7 roles; Mocha defaults mirroring strop's quiet/loud
tiers; other palettes derive suitable tints, and explicit overrides win). Gutter
`[sign][old][new][content]`, `▸` cursor marker, structural bands,
scrollbar per house rules; sanitize at the boundary (patch text is
network text).

### M5 — tests + docs + release

Render tests (dive frames, emphasis spans via Buffer cells, band
colors, no-lingering on unwind); headless e2e on fs_provider serving
real git history (script: history → d → Enter → ]f → Esc ladder);
CHANGELOG + site roadmap/changelog; demo tape gains a commit-dive
beat. Release v0.10.0 (0024's split + v1.6 protocol + this).

## Not doing

- No side-by-side, syntax-in-diff, or context folding (strop's
  recorded pushbacks, adopted).
- No staging/revert/patch-export (0016 boundary).
- No repo-wide log entry point yet (trait takes `path: Option<&str>`
  already; the UI surfaces it when a demand shows up — roadmap).
- No commit-graph lane rendering in the history lens (strop colors
  graph runes; rootle's history rows are flat two-line rows — revisit
  with repo-wide log).
