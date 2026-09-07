# House style — the component behavior contract

Written from the code; this is the checklist new components are
reviewed against. Where a plan disagrees, the code wins.

## Component trait

`crates/rootle/src/components/mod.rs`: a component owns its state, renders into a
caller-given `Rect`, and never mutates the app directly —

```rust
fn handle_key(&mut self, key: KeyEvent) -> Action;
fn update(&mut self, action: &Action);
fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme);
```


## Component layout: file per component, sibling submodules

One component = one file in `crates/rootle/src/components/` (helix's `ui/`, gitui's
`popups/` convention). A cohesive component of a few hundred lines is
fine there. When a component carries several concerns, the file stays
as the public surface — state struct, `update`, accessors — and
private submodules hang off the sibling directory:

```
global_search.rs        state + update + re-exports
global_search/keys.rs   handle_key + focus/scope/filter logic
global_search/render.rs drawing (fields row, result blocks, popup)
global_search/model.rs  SearchKind/Scope/SearchHit data model
global_search/backend.rs worker-side search (run_view_search & co)
global_search/mock.rs   the offline producer (pub mod — app injects it)
```

The same rule decomposes `settings_popup` (sections/render),
`preview` (find/render), `browser` (navigation/revisions/marks), and
`rootle-stdio` (routing/reader/transport/restart). Split along concern
seams (keys vs render vs model vs worker I/O), never mechanically; tests live next to the code they
exercise. Private struct fields stay private — submodules are
descendants and see them; nothing outside the component does.

## Action flow is unidirectional

Keys become `Action`s (`crates/rootle/src/action.rs`); `App::handle_action`
(`crates/rootle/src/app/actions/`) dispatches cross-component app state changes.
Components return Actions, including follow-ups: `browser.update`
returns the action the move implies (e.g. selection changed → load
blob), and `handle_action` routes it. Worker threads never touch the
UI; results return as `AppEvent`s over an mpsc channel, are converted
to Actions, and domain-tagged request generations drop stale replies.
Advisory cancellation is not a freshness guarantee. Clipboard writes and editor spawns are queued
(`take_clipboard`, `take_editor_job`) and executed by the main loop
outside the draw path.

## Modal text input: VimInput

`crates/rootle/src/components/vim_input.rs` — one widget for every text field.
Focus always lands in INSERT. In a **modal** input (popup queries,
search-view fields), Esc drops to NORMAL: `h`/`l`/`0`/`$` move, `x`
deletes, `i`/`a`/`A` re-enter INSERT; a second Esc returns
`Outcome::Cancelled` and the owner decides what dismissal means. In a
**transient** input (`VimInput::transient()` — `/` filter lines,
settings field edits), Esc cancels directly, like vim's `/`.
`prefill()` seeds a replaceable value (resume flows): the first edit
clears it, Enter submits it unchanged. Cursor shape follows the
submode: bar in INSERT, block in NORMAL, hidden otherwise
(`cursor_style()`).

## `/` filters on every results pane

Browser panes, the search popup's results, and the global search
results all share one filter contract: `/` starts a transient
incremental session — keystrokes filter live (case-insensitive
substring), **Enter commits** (filter stays applied), **Esc cancels**
(restore the pre-session value). With a committed filter, the first
Esc clears it; the next Esc closes/closes the pane (`pane.rs` title
shows `title /filter`; `search_popup.rs`, `global_search.rs`). **Any
list a user scans should be filterable** — this includes wizard lists
(clone repos, destination folders: `clone_wizard.rs`); a scrollable
list without `/` is a style bug.

`components/list_view` owns `ListFilter`, `ListCursor` and `Viewport`.
An item index is not a display-row offset: headers and multi-line rows
must not alter which entry is selected. Refs, help, settings, clone
lists, history and commit files share these mechanics. Preserve the
selected entity, not its former numerical position, when data refreshes.

## Scrollbars

Any content that scrolls shows one: the track is the right border
itself (`│`, `surface2`), the thumb a bold accent column (`┃`,
`border_focused`) — `components::scrollbar` (`mod.rs`), called with
(outer rect, content height, total lines, offset). Nothing renders
when content fits. No separate scrollbar column is ever allocated —
the bar lives inside the border, and content never shifts.

## Popup shell rules

- **One deep.** `App` holds at most one overlay of each kind and
  never nests popups (`app/mod.rs`: `Option<SearchPopup>`, `help`,
  `settings`, `wizard`, `command_line`). The scope radio popup inside
  the search view is the single sanctioned inner popup.
- **Centered + `Clear` while open.** Popups render `Clear` first so no
  underlying cells linger (`components::centered`, `search_popup.rs`).
- **No clear on close.** Closing a popup only drops it; the next frame
  redraws. A full `terminal.clear()` has exactly one trigger — editor
  resume (`app/mod.rs: force_redraw`, `main.rs`).
- **Radio follows the cursor.** In the scope popup (`global_search.rs`),
  `j/k/g/G` move the cursor and apply the scope it lands on live;
  Enter commits by closing, Esc reverts to the pre-popup value.
  Disabled rows render dim and are skipped.
- Popup borders carry the mode-specific hint row (`title_bottom`),
  e.g. `tab focus · enter submit/select · / filter · esc close`.

## Selection styling

Selected rows get `selection_bg` background + `selection_fg` foreground
and a `▌` gutter symbol (`pane.rs`: `highlight_symbol("▌")`;
`global_search.rs` path rows prepend a `"▌ "` span). The gutter is a
separate span — selection never flips bold or restyles content beyond
fg/bg. VISUAL marks are a `●`/`○` gutter column (`pane.rs`), colors
from the theme.

## Modeline contract

One bottom line (`crates/rootle/src/components/modeline.rs`):
mode chip → forge chip → vim-style caret → transient status → context
→ `? keys` affordance. The modeline describes state, not keybindings.
Segment arrows bridge colors (fg = the segment being left, bg = the
segment being entered). Nerd Font glyphs (`[ui] nerd_font = true`)
are opt-in; text-only chips and `❯` work without a patched font.

There is one hint surface per keyboard owner. Popups use their bottom
border; browser transient modes and the leader layer use the glued
strip. Tables supply both dispatch and hints. `effective_mode()` follows
the topmost input owner, including active filters and field edits.
Every segment is width-fitted; status/context truncation must leave the
modeline on the final row even at tiny viewport sizes.

## Keymap tables are the single source of truth

`crates/rootle/src/keymap.rs` and its siblings hold the binding
tables for modes, list contexts, search fields and motion commands.
Dispatch and hints derive from the same rows. Stateful sequences such
as `]f`/`[f` consume those definitions; they are not single-key aliases.
The `?` popup renders the tables, never a separately maintained list.
Command-line options derive from `commands::COMMANDS` in
`crates/rootle/src/commands.rs`.

## Provider seam

The UI never talks to a concrete backend; it talks to `trait Provider`
(`crates/provider/src/lib.rs`), always on worker threads. App code imports
the vocabulary directly from `rootle_provider`; the app's `provider/`
module owns composition and configuration, not compatibility re-exports.
`RepoId`, `Sha` and `GitRef` remain opaque; display sanitization must not
change an identifier used for provider calls. URL building stays in the
provider. External backends use `crates/stdio/` for NDJSON-RPC — see
[provider-protocol.md](provider-protocol.md). Provider
misconfiguration falls back to GitHub with a status warning; it never
blocks startup.

## Sanitize at the boundary

Everything drawn from the network passes `crates/rootle/src/sanitize.rs`
where it enters UI state (`app/actions/`, `app/events.rs`): binary
detection (NUL or >10% control bytes in the first 8 KiB → binary
placeholder), lossy UTF-8, and control-strip that removes ESC so file
content can't inject terminal sequences. Single-line names use
`sanitize_inline`. Highlighting (syntect) happens once at the same
boundary, on the UI thread.

## Width-correct truncation

All truncation is by display width (`pane::fit`, `unicode_width`),
never by byte or char count — CJK glyphs occupy two cells. Popups and
the modeline compute padding from `UnicodeWidthStr::width` of rendered
spans, so the layout survives wide characters.

## Typed identity, no naked strings (plans/0025)

Identity crosses a boundary only as its newtype from
`rootle_provider::id`: `OrgId`, `RepoId`, `RepoPath`, `Sha` and `GitRef`.
The provider seam distinguishes repository, revision and content identity.
Async staleness uses domain-tagged `Generation<Domain>` values
(`tick()` / `is_current()`); unrelated pipelines cannot compare clocks.
VISUAL `MarkKey` variants identify organizations, repositories or
repository/ref/path entries. Captions, column positions and concatenated
`"<title>/<name>"` strings are not persistent identity.
Name domain coordinates such as item indices and display offsets;
local arithmetic and explicit layout constants do not need wrappers.

## File size ceiling

~400 lines is healthy, ~800 is the ceiling — then split by concern
into sibling submodules (the `settings_popup/{mod,render,sections}`
and `global_search/` shapes). The exceptions are single-pattern
artifacts (the keymap tables) where splitting would obscure the
pattern.
