# 0026 — Unified components: one list widget, one dispatch table

Status: **implemented and verified locally (2026-09-07)** — refs, help,
settings, clone lists, history and commit files share `ListFilter`,
`ListCursor` and `Viewport`. Item and display coordinates are separate;
binding tables drive both commands and hints. The former migration
deferral is closed by 0029. Release tracking lives there.

## Problem

Two divergences, both named in the code:

1. **List rendering.** Only `pane.rs` uses ratatui `List`/`ListState`.
   Seven other surfaces hand-roll `Vec<Line>` + `Paragraph::scroll` +
   `cursor: usize`/`scroll: u16`: refs_popup (235–241), keybinds_popup
   (195–199), settings_popup/render (128–140), clone_wizard/render
   (90–102), browser/lenses history rows (386–398, two-line rows),
   global_search file pane, preview scroll clamp. The keep-cursor-
   visible snippet is copy-pasted ≥5×. Every new list (0028's changed
   files among them) re-derives filter/scroll/scrollbar/selection —
   the strop "duplicated per-pane render loop" smell, pre-split.
2. **Keymap divergence.** keymap.rs:66–72 records it: preview and
   search-view dispatch "lives with the component" while hints stay
   in the table — the table is supposed to be the single source
   (house-style); today it is half-true.

strop's answers: one BINDINGS table IS dispatch + help + which-key
(tables carry `id` + `live` flags); one text renderer for all panes
(their 0010 decision 3 deleted the drifted copy and inactive panes
"gained diff rows for free").

## Decisions

1. **`ListView`** (components/list_view.rs): the shared scrollable
   list — rows (`Vec<Line>` or two-line row pairs), cursor +
   keep-visible scrolling, optional `/` filter session (the house
   contract, built in: incremental, Enter commits, Esc restores,
   committed-filter Esc clears), border-embedded scrollbar, selection
   styling (`selection_bg/fg` + `▌` gutter), focus dimming, CJK-safe
   truncation via pane::fit. Millers columns (`pane.rs`) stay as-is:
   ratatui List with badges/checkboxes is a different widget; `Pane`
   keeps its crown.
2. **Migrations, one commit each, render-parity gated**: refs_popup,
   keybinds_popup, settings list, clone_wizard lists, history lens
   (row_height = 2). global_search's variable-height hit boxes are NOT
   a ListView (boxes ≠ rows) — untouched.
3. **Keymap closure**: preview/search-view/history named-key tables
   become THE dispatch path (components consult the table; hints
   derive from the same rows — deleting the "dispatch lives with the
   component" exception). New surfaces (0028) register keys in
   keymap.rs first, per the component skill checklist.
4. Tests: for each migration, before/after frame equality on the
   existing render fixtures (the 57-test suite already covers these
   surfaces); a ListView unit block for filter/scroll/keep-visible
   edge cases (top/bottom clamp, short viewport, two-line rows).

## Not doing

- No trait redesign — `Component` stays as house-style defines it;
  ListView is a library component others compose, like VimInput.
- No which-key/pending-key popup machinery (strop's BINDINGS scale);
  rootle's modes are flatter. Revisit if modes grow pending keys.

## Verification

`docker compose run --build --rm test` + `e2e`; frame-diff check per
migration via `--headless` scripts on the fs provider.
