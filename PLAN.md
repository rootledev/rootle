# ghx — Plan

A modal ratatui TUI for browsing remote GitHub repos with a yazi-like flow.
No local clone required: the backend is the GitHub REST API, with a
content-addressable disk cache under `~/.cache/ghx`.

## 1. UX Flow

```
launch
  └─▶ SearchPopup (input field on top, results list below)
        │  lands in input, INSERT mode — type immediately
        │  <Enter>  → submit query to GitHub repo search API,
        │             focus results list
        │  (no live API calls while typing — Enter is the trigger;
        │   incremental filtering is reserved for pane SEARCH)
        │  <Tab>    → toggle focus input ⇄ results
        │             (focusing input always lands in INSERT mode,
        │              so a typo fix is: <Tab> → type → <Enter>)
        │  ↑↓/j/k   → navigate results
        │  /        → local incremental filter over the result set
        │             (SEARCH chip; type filters live, <Enter> commits,
        │              <Esc> cancels to pre-filter; pure local, no API)
        │  <Esc>    → input: INSERT→NORMAL; NORMAL: dismiss popup
        │  <Enter> on result → select repo
        ▼
  Browser (three-pane miller columns over the hierarchy
  org → repo → dir → … → file; yazi rule: left = parent level with the
  current selection highlighted, center = current level, right = preview)
        ┌──────────┬──────────┬──────────────┐
        │ org      │ repo     │ preview      │
        │ repos    │ tree     │ (file:       │
        │ (parent) │ (current │  highlighted │
        │          │  dir)    │  dir: tree)  │
        └──────────┴──────────┴──────────────┘
  ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄ modeline + key hints ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄
        l/→  drill in (repo → root dir → subdir → …) or move focus right
        h/←  move focus LEFT into the parent column — the parent stays
             live: j/k browse it and the child column rebuilds
             (cascades) from the new selection, yazi-style
        j/↓  next entry       k/↑  prev entry
        /    enter SEARCH mode on the focused pane: incremental filter
             while typing; <Enter> commits filter (back to BROWSE,
             filter stays); <Esc> while typing cancels to pre-filter
             state; <Esc> on a committed filter clears it
        <Enter> on file → open in editor (read-only), exit → back to ghx
        <Space> leader → command layer (search, reload, quit, ...)
        q    quit
```

## 2. Modes & modeline

ghx is strictly modal. Global mode stack; the modeline always shows the
current mode as a chip, plus context key hints on the right.

| Mode     | Modeline chip | Entered by | Exits to |
|----------|---------------|------------|----------|
| BROWSE   | `[BROWSE]`    | default after repo select | — |
| SEARCH   | `[SEARCH]`    | `/` on any pane or popup results | BROWSE on `<Enter>`/`<Esc>` |
| INSERT   | `[INSERT]`    | any focused text field (popup query, `/` filter line) | NORMAL on `<Esc>` (modal inputs) |
| NORMAL   | `[NORMAL]`    | `<Esc>` in a modal text input | INSERT on `i`/`a`; popup closes on `<Esc>` |
| LEADER   | `[LEADER]`    | `<Space>` | previous mode after command |
| VISUAL   | `[VISUAL]`    | later phases (multi-select) | — |

- `/` filter lines are *transient* inputs: no NORMAL sub-mode — a single
  `<Esc>` cancels, like vim's own `/`. Only standalone fields (the popup
  query) get the full INSERT/NORMAL treatment.

- Modeline: one row at the very bottom, full width. Left: mode chip
  (accent-colored, bold). Middle: context path or status (current repo,
  `owner/repo:path`, rate-limit badge). Right: key hints for the current
  mode (`j/k move · / filter · ␣ leader · q quit`).
- Popups carry their own hint row at the popup's bottom border; popups
  are always dismissed with `<Esc>` (from NORMAL sub-mode in an input,
  directly from a list).
- Every text input is a mini-vim: focus lands in INSERT (cursor visible,
  type immediately). `<Esc>` → NORMAL with simple motions (`h l` move
  char, `0 $` line bounds, `x` delete char; `w b` and `dd` are stretch
  goals, not v1). `i`/`a` return to INSERT. `<Enter>` in INSERT is the
  trigger (submit query / commit filter).
- SEARCH on panes is **incremental**: filter applies per keystroke, no
  trigger needed. This differs from input submission, where `<Enter>` is
  the trigger — intentional asymmetry, matches the flow above.

## 3. Config (`~/.config/ghx/config.toml`)

Loaded via the `dirs` crate (respects XDG; `$GHX_CONFIG` overrides).
Missing file → defaults, file is written on first run so users discover it.

```toml
[editor]
program = "hx"          # or "vim"; falls back to $VISUAL/$EDITOR probe
args = []               # extra args before the file path
read_only = true        # adds -R for vim where supported

[theme]
name = "catppuccin-mocha"   # resolved against theme dirs
# path = "/abs/or/~/theme.toml"  # explicit path overrides name
```

A settings view (edit config in-app) is a later phase; v1 is file-only.

## 4. Theming

- **Palette is external to the app.** Themes are TOML palette files.
  Resolution order for `[theme].name`: `~/.config/ghx/themes/<name>.toml`
  → `<config dir>/themes/<name>.toml` → embedded fallback.
- Default: **Catppuccin Mocha**, embedded as the fallback so the binary
  runs standalone; on first run ghx also writes
  `~/.config/ghx/themes/catppuccin-mocha.toml` so users can fork it.
- Palette schema (all keys required; hex strings):

```toml
[base]      # crust/mantle/base/surface0..2, overlay0..2, text, subtext0/1
[accent]    # mauve, blue, teal, green, yellow, peach, red, pink, ...
[semantic]  # mapped roles: border_focused, border_unfocused, directory,
            # file, selection_bg, selection_fg, mode_browsing, mode_search,
            # mode_insert, mode_normal, error, warning, hint, highlight_*
```

- Syntax highlighting maps syntect scopes onto the palette's semantic
  roles (a small fixed mapping, not per-theme scope files, for v1).

## 5. Design language

Principles: quiet by default, one accent, state lives in the modeline.

- **Layout**: full-bleed panes, no padding inside list rows, single-char
  left gutter holding the selection indicator. Three panes at fixed
  ratios 1 : 2 : 2 (parent : current : preview).
- **Borders**: rounded (`border_type::Rounded`). Focused pane border in
  `border_focused` (blue — the dominant accent); unfocused in `border_unfocused` (surface2).
  Pane title sits in the top border, left-aligned: the pane's path
  (`src/`, `ratatui/src/widgets/`).
- **Popups**: centered, ~60% width, dim backdrop (render main UI, overlay
  a translucent-block `Clear` rect). Rounded border, title top-left,
  hint row embedded in the bottom border (`tab focus · enter select · esc close`).
  Never more than one popup deep.
- **Files vs directories**: directories in `directory` (blue, bold) with
  a trailing `/`; files in `file` (text). Executables in green and
  symlinks in teal are stretch roles already in the palette schema.
- **Selection**: row gets `selection_bg` (surface0) full-width +
  `selection_fg` text; the gutter indicator is a `▌` in accent. No
  bold-flipping of the row text — background carries the state.
- **Modeline**: inverse surface bar; mode chip is a solid accent block
  with crust text, one per mode (browse=green, search=yellow,
  insert=teal, normal=blue, leader=peach, visual=pink). Hints are
  `subtext0` with the key itself in `text` bold.
- **Feedback**: network in-flight → spinner in the modeline right of the
  mode chip; errors → a transient toast line above the modeline
  (auto-clears on next successful action), never a blocking dialog for
  recoverable failures.
- **Typography**: no emoji, no nerd-font dependency for core UI; icons
  opt-in via config later.

## 6. Architecture

Pattern: **Component architecture** (per ratatui's documented patterns),
with a unidirectional `Action` flow borrowed from Flux:

- One event loop. Crossterm events + async app events (API responses,
  cache writes) are unified into a single `Event` stream via
  `tokio::sync::mpsc` + `crossterm::event::EventStream` + `tokio::select!`.
- Every UI unit implements a `Component` trait:
  `handle_events -> Action`, `update(Action) -> Action`, `render(Frame, Rect)`.
- `Action` is a central enum (`SearchSubmitted`, `RepoSelected`,
  `DirLoaded`, `ModeChanged`, `MoveUp`, `OpenEditor`, `Quit`, ...).
  Components never call each other; they emit actions the root
  dispatcher routes.
- A `VimInput` widget (state machine: Insert/Normal, cursor, motions)
  is shared by every text field — popup search input and pane filter
  line use the same component with different trigger wiring.
- The keymap is a data table (`mode → key → Action`), not nested
  matches, so the modeline hints and the help popup are *derived* from
  the same source of truth as the dispatch itself.

### Module layout

```
src/
  main.rs            // terminal setup/teardown, panic hook
  app.rs             // root: mode stack, action dispatch, component tree
  event.rs           // Event enum, crossterm + tokio mpsc merge
  action.rs          // Action enum
  mode.rs            // Mode enum
  keymap.rs          // mode → key → Action table; drives dispatch AND hints
  components/
    search_popup.rs  // VimInput + results, Tab focus switching
    browser.rs       // owns the three panes
    pane.rs          // one yazi pane (list state, filter buffer)
    preview.rs       // file preview / dir summary
    vim_input.rs     // modal text field (INSERT/NORMAL, motions)
    modeline.rs      // mode chip, status, key hints
    help.rs          // leader-key cheatsheet popup
  github/
    client.rs        // thin wrapper: search, repo meta, trees, blobs
    types.rs         // Repo, TreeEntry, ... (serde models)
  cache/
    mod.rs           // Cache handle, layout, atomic writes
    index.rs         // repo ref → sha resolution cache
  theme.rs           // palette loading, semantic roles, syntect mapping
  highlight.rs       // syntect wrapper driven by theme.rs
  editor.rs          // suspend/resume terminal, spawn editor
  config.rs          // ~/.config/ghx/config.toml (serde + defaults)
```

### Rendering notes

- Render on demand (state change or tick for spinners), not every loop.
- Popups render as a final overlay layer over the dimmed main UI.
- Preview highlighting runs once per file, result stored in an
  `LruCache<BlobSha, Vec<Line>>` in memory; re-highlight only on theme
  change.

### Reusable component library

Ratatui ships widgets, not app-level components — there is no React-style
equivalent built in. Our `Component` trait **is** that layer, and we keep
a small library of reusable, self-contained components under
`src/components/`. Rule: a component appears once in the library and is
*instantiated* wherever needed — no per-screen copies of inputs or lists.

| Component | Reused by |
|-----------|-----------|
| `VimInput` (modal text field) | search popup query, pane `/` filter, future settings fields |
| `ListView` (scrollable selectable list w/ optional filter) | search results, every miller pane |
| `Popup` (shell: centered rect, Clear, rounded border, title + hint row) | search popup, help, future dialogs |
| `Pane` (border + title + ListView + filter line) | the three miller columns |
| `Modeline` (mode chip + status + hints) | global |
| `Preview` (sanitized, highlighted text viewer) | right column |

Component contract: each owns its state, returns `Action` from
`handle_key`, mutates only via `update(Action)`, renders into a caller-
given `Rect`. Parent components compose children and forward events
based on focus — children never know who embeds them.

## 7. GitHub backend

- Auth token resolution order: `GHX_TOKEN` → `GITHUB_TOKEN` → `gh auth token`
  (shell out) → anonymous (60 req/h, warning badge in modeline).
- Endpoints:
  - `GET /search/repositories?q=...` — search popup, fired on `<Enter>`
    (no per-keystroke calls; keeps rate-limit spend explicit).
  - `GET /repos/{owner}/{repo}` — default branch + head sha.
  - `GET /repos/{owner}/{repo}/git/trees/{sha}?recursive=1` — fetch the
    **whole tree once** per ref (cap at the API's 100k-entry / 7MB
    truncation limit; fall back to per-dir tree calls when truncated).
    One request upfront makes all directory navigation instant and
    offline-capable via cache, vs a round trip per `l`.
  - `GET /repos/{owner}/{repo}/git/blobs/{sha}` — file content (base64),
    fetched lazily on preview, keyed by blob sha in cache.
- Conditional requests: send `If-None-Match` with stored ETags; a `304`
  costs nothing against the rate limit and validates the cache.
- Rate-limit state (`X-RateLimit-Remaining`) surfaced in the modeline.

## 8. Cache design (`~/.cache/ghx`)

Content-addressable, keyed by the git sha values the API already returns.
Blobs and trees dedupe across repos for free (same blob sha ⇒ same bytes).

```
~/.cache/ghx/
  index/
    repos.json                    # recent repos, last-selected, LRU of searches
    refs/<owner>/<repo>/<branch>  # → { "tree_sha": "...", "etag": "...", "fetched_at": ... }
  trees/
    <tree_sha>.json               # full recursive tree listing
  blobs/
    <ab>/<cdef...>                # raw file bytes, fan-out by first 2 chars
  edit/
    <repo-slug>/<path>            # materialized copies for the editor
  meta/
    blobs.json                    # sha → { size, last_access } for eviction
```

Rules:

- **Blobs/trees are immutable** (sha-keyed) — never invalidated, only evicted.
- **Ref → sha resolution is mutable** — cached with ETag, revalidated on
  repo open; on `304` the whole cached tree is still valid because the
  head sha is unchanged.
- **Atomic writes**: write to `*.tmp` then `rename`, so a kill mid-write
  never yields a corrupt blob.
- **Eviction**: size cap (default 512MB, configurable); evict blobs by
  LRU via `meta/blobs.json`; orphan sweep on startup removes blobs/trees
  not referenced by any cached ref.
- Fan-out depth: 1 level (`ab/`) is sufficient for the expected scale.

## 9. Text safety & rendering correctness

Remote file bytes are hostile input for a terminal. Rules:

- **Sanitize before render, once, at the cache boundary** — everything
  the UI draws (preview, file names in trees) passes through
  `sanitize.rs` first:
  - Decode with `String::from_utf8_lossy` (invalid sequences → ``,
    never panic, never pass raw bytes to the terminal).
  - Strip all C0/C1 control chars except `\n` and `\t`; **especially
    `\x1b`** — a single ESC in file content would inject terminal
    escape sequences and corrupt the whole screen. Tabs expand to 4
    spaces at render time.
  - **Binary detection**: NUL byte or >10% control chars in the first
    8 KiB ⇒ not text. Preview renders a placeholder
    (`binary file · N bytes · sha …`), never the bytes.
  - File names from the API get the same treatment before hitting a
    `List` item.
- **Width correctness**: all truncation/padding via `unicode-width`
  (CJK is 2 cells, combining marks 0) — never `str::len()` for layout.
- **Redraw integrity**:
  - ratatui's double buffer diffs cells, so normal updates don't leave
    residue — *except* when content shrinks behind an overlay. Every
    popup therefore renders `Clear` first (resets cells), and closing a
    popup forces a full redraw.
  - Full redraw (`terminal.clear()`) on: resize, editor resume, popup
    close. These are the only three paths where stale cells can linger.
  - No partial `stdout` writes outside the draw path; the panic hook
    restores the terminal before printing anything.

## 10. State store (persistence)

Distinct from the cache: cache is evictable, state is not.

`~/.local/state/ghx/state.json` (XDG state dir; small, human-readable,
written atomically tmp+rename, debounced ~500ms after change):

```json
{
  "version": 1,
  "last_org": "ratatui",
  "last_repo": { "owner": "ratatui", "name": "ratatui" },
  "last_path": "src/widgets",
  "recent_repos": [ "ratatui/ratatui", "helix-editor/helix" ],
  "recent_orgs": [ "ratatui", "tokio-rs" ]
}
```

- On launch with a `last_repo`, ghx offers "resume" via the search
  popup's prefilled state (still lands on the popup; one Enter to resume).
- Recents cap at 20, LRU order, deduped. Recent orgs feed the first
  pane even before any search (browse an org without searching).
- Corrupt/missing state file → defaults, never a startup failure.
- Schema `version` field for future migrations.

## 11. Syntax highlighting

`syntect` with the default sublime-syntax dumps (pure Rust, musl-static
friendly), scopes mapped onto the active palette's semantic roles.
File type detection by extension + first-line fallback.
Alternative considered: tree-sitter — more accurate, but C compilation
per grammar complicates the static musl build; syntect is sufficient.

## 12. Editor integration

On `<Enter>` over a file:

1. Materialize the blob from cache (or fetch) into
   `~/.cache/ghx/edit/<repo-slug>/<path>` so the user sees a real filename.
2. Suspend: leave alternate screen, disable raw mode.
3. Spawn editor **read-only** (decided: ghx is a browser/viewer — `vim -R`,
   `-m` fallback note in docs for editors without a read-only flag), wait.
   Resolution order: `[editor].program` → `$VISUAL` → `$EDITOR` → probe
   `hx`, `nvim`, `vim`, `vi`.
4. Resume: re-enter alternate screen, raw mode, force full redraw.

## 13. Build & release

- Target: `x86_64-unknown-linux-musl` (plus `aarch64` later), fully static.
- `Dockerfile`: multi-stage — `rust:alpine` builder stage compiles with
  `cargo build --release --target x86_64-unknown-linux-musl`
  (strip, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`);
  final stage is `scratch` holding only the binary.
- `docker-compose.yml` services:
  - `test`    → `cargo test` + `cargo clippy -- -D warnings` + `cargo fmt --check`
  - `build`   → debug musl build for local iteration
  - `release` → stripped static release binary exported to `./dist/`
  - Usage: `docker compose run test`, `docker compose run release`.
- Release flow: `git tag vX.Y.Z` → CI (GitHub Actions) runs
  `docker compose run release` → `gh release create vX.Y.Z dist/ghx-*`
  with generated notes + sha256 checksums. `cargo-release` for version
  bumping. `cargo-dist` later if we want installers/Homebrew — not v1.
- Verification gate before publishing: run the scratch image binary with
  `--version` in CI to prove static linking (`ldd` says "not a dynamic
  executable").

## 14. Testing

- Unit: cache layout/atomic write, tree parsing, filter logic, keymap
  table ↔ hint derivation consistency, VimInput mode transitions.
- Component: ratatui `TestBackend` + buffer assertions for popup focus
  switching (Tab round-trip), pane filter incremental behavior, modeline
  chip per mode; `insta` snapshots for the three-pane layout.
- GitHub client: `wiremock` against recorded API payloads; 304/ETag and
  rate-limit paths covered.
- Theme: palette TOML round-trip, missing-key errors, embedded fallback
  loads when no files exist.
- Editor suspend/resume: integration test behind a flag (needs a TTY),
  otherwise smoke-tested manually.

## 15. Milestones

UI-first: get something on screen immediately, tweak the design language
against a real render, wire the network last. Small incremental commits
per milestone; each milestone is shown (text snapshot + live PTY run)
before moving on.

1. **Static UI shell** — terminal lifecycle, component trait, mode
   stack, modeline, keymap table, Catppuccin Mocha, and the full layout
   (three panes + search popup) rendered from **mock data**. Navigation,
   `/` filter, Tab focus, VimInput all work locally. No network.
   → design-language review checkpoint.
2. **State store** — `state.json` persistence (last repo/org, recents),
   resume flow in the popup.
3. **GitHub search** — real repo search in the popup (Enter-triggered),
   org repos endpoint feeding pane one.
4. **Tree browsing** — recursive tree fetch, cache, drill in/out across
   the miller columns.
5. **Preview** — blob fetch, sanitization, syntect highlighting.
6. **Editor** — suspend/resume + read-only spawn.
7. **Cache hardening** — ETag revalidation, eviction, orphan sweep.
8. **Release pipeline** — docker, compose services, gh release, CI.
9. **Later** — VISUAL mode (multi-select), settings view, icons.
