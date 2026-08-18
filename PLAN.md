# ghx — Plan

A modal ratatui TUI for browsing remote GitHub repos with a yazi-like flow.
No local clone required: the backend is the GitHub REST API, with a
content-addressable disk cache under `~/.cache/ghx`.

## 1. UX Flow

```
launch
  └─▶ SearchPopup (input field + results list below)
        │  type query → debounce → GitHub repo search API
        │  <Enter>    → move focus from input to results list
        │  ↑↓/j/k     → navigate results
        │  <Enter>    → select repo
        │  <Esc>      → quit (or back to browser if a repo was open)
        ▼
  Browser (three-pane yazi layout)
        ┌──────────┬──────────┬──────────────┐
        │ parent   │ current  │ preview      │
        │ dir      │ dir      │ (file:       │
        │ (left)   │ (center) │  highlighted │
        │          │          │  dir: tree)  │
        └──────────┴──────────┴──────────────┘
        l/→  enter dir        h/←  go to parent
        j/↓  next entry       k/↑  prev entry
        /    live-filter current pane (type → filters, <Enter> commits
             filter, <Esc> clears committed filter)
        <Enter> on file → open in $EDITOR (vim/helix), on exit → back to ghx
        <Space> leader → command layer (back to search, reload, quit, ...)
        q    quit
```

Modal: modes are `Search`, `Browse`, `Filter` (a sub-mode of Browse), and
`Leader` (awaiting the key after `<Space>`). Mode determines which component
owns key events and how the status bar renders.

## 2. Architecture

Pattern: **Component architecture** (per ratatui's documented patterns),
with a unidirectional `Action` flow borrowed from Flux:

- One event loop. Crossterm events + async app events (API responses,
  cache writes) are unified into a single `Event` stream via
  `tokio::sync::mpsc` + `crossterm::event::EventStream` + `tokio::select!`.
- Every UI unit implements a `Component` trait:
  `handle_events -> Action`, `update(Action) -> Action`, `render(Frame, Rect)`.
- `Action` is a central enum (`SearchSubmitted`, `RepoSelected`,
  `DirLoaded`, `MoveUp`, `OpenEditor`, `Quit`, ...). Components never call
  each other; they emit actions the root dispatcher routes.
- **Why not Elm/TEA**: a single global `update` gets unwieldy once the
  filter sub-mode, popup, and three panes each need private state
  (scroll offsets, filter buffers, preview caches). Components keep that
  co-located; the shared `Action` enum keeps it from turning into
  callback spaghetti.

### Module layout

```
src/
  main.rs            // terminal setup/teardown, panic hook
  app.rs             // root: mode stack, action dispatch, component tree
  event.rs           // Event enum, crossterm + tokio mpsc merge
  action.rs          // Action enum
  mode.rs            // Mode enum + per-mode keymap table
  components/
    search_popup.rs  // input + results, two focus targets
    browser.rs       // owns the three panes
    pane.rs          // one yazi pane (list state, filter buffer)
    preview.rs       // file preview / dir summary
    status_bar.rs    // mode, key hints, rate-limit badge
    help.rs          // leader-key cheatsheet popup
  github/
    client.rs        // thin wrapper: search, repo meta, trees, blobs
    types.rs         // Repo, TreeEntry, ... (serde models)
  cache/
    mod.rs           // Cache handle, layout, atomic writes
    index.rs         // repo ref → sha resolution cache
  highlight.rs       // syntect wrapper, theme loading
  editor.rs          // suspend/resume terminal, spawn $EDITOR
  config.rs          // keybindings, theme, editor override (TOML)
```

### Rendering notes

- Render on demand (state change or tick for spinners), not every loop.
- Popups render as a final overlay layer over the dimmed main UI
  (`Clear` + centered rect), yazi-style.
- Preview highlighting runs once per file, result stored in an
  `LruCache<BlobSha, Vec<Line>>` in memory; re-highlight only on theme
  change.

## 3. GitHub backend

- Auth token resolution order: `GHX_TOKEN` → `GITHUB_TOKEN` → `gh auth token`
  (shell out) → anonymous (60 req/h, warning badge in status bar).
- Endpoints:
  - `GET /search/repositories?q=...` — search popup. Debounce ~300ms,
    cancel in-flight on new keystroke (tokio `select!` / abort handle).
  - `GET /repos/{owner}/{repo}` — default branch + head sha.
  - `GET /repos/{owner}/{repo}/git/trees/{sha}?recursive=1` — fetch the
    **whole tree once** per ref (cap at the API's 100k-entry / 7MB
    truncation limit; fall back to per-dir tree calls when truncated).
    This is the key latency decision: one request upfront makes all
    directory navigation instant and offline-capable via cache, vs the
    contents-API-per-directory approach which pays a round trip per `l`.
  - `GET /repos/{owner}/{repo}/git/blobs/{sha}` — file content (base64),
    fetched lazily on preview, keyed by blob sha in cache.
- Conditional requests: send `If-None-Match` with stored ETags; a `304`
  costs nothing against the rate limit and validates the cache.
- Rate-limit state (`X-RateLimit-Remaining`) surfaced in the status bar.

## 4. Cache design (`~/.cache/ghx`)

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
- Open question worth revisiting: fan-out depth and whether the meta
  index should be a small sled/sqlite DB if JSON gets slow past ~10k blobs.

## 5. Syntax highlighting

`syntect` with the default sublime-syntax dumps (pure Rust, musl-static
friendly). File type detection by extension + first-line fallback.
Theme: one dark + one light embedded, overridable in config.
Alternative considered: tree-sitter — more accurate, but C compilation
per grammar complicates the static musl build; syntect is the boring,
sufficient choice.

## 6. Editor integration

On `<Enter>` over a file:

1. Materialize the blob from cache (or fetch) into
   `~/.cache/ghx/edit/<repo-slug>/<path>` so the user sees a real filename.
2. Suspend: leave alternate screen, disable raw mode.
3. Spawn editor, wait: resolution order `$GHX_EDITOR` → `$VISUAL` →
   `$EDITOR` → probe `hx`, `nvim`, `vim`, `vi`.
4. Resume: re-enter alternate screen, raw mode, force full redraw.

Save semantics are TBD (see open questions — read-only vs commit-back).

## 7. Build & release

- Target: `x86_64-unknown-linux-musl` (plus `aarch64` later), fully static.
- `Dockerfile`: multi-stage — `rust:alpine` (or `clux/muslrust`) builder
  stage compiles with `cargo build --release --target x86_64-unknown-linux-musl`
  (strip, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`);
  final stage is `scratch`/distroless holding only the binary.
- `docker-compose.yml` services:
  - `test`    → `cargo test` + `cargo clippy -- -D warnings` + `cargo fmt --check`
  - `build`   → debug musl build for local iteration
  - `release` → stripped static release binary exported to `./dist/`
  - Usage: `docker compose run test`, `docker compose run release`.
- Release flow: `git tag vX.Y.Z` → CI (GitHub Actions) runs
  `docker compose run release` → `gh release create vX.Y.Z dist/ghx-*`
  with generated notes + sha256 checksums. `cargo-release` for version
  bumping. Optionally `cargo-dist` later if we want installers
  (`ghx-installer.sh`, Homebrew tap) — not for v1.
- Verification gate before publishing: run the scratch image binary with
  `--version` in CI to prove static linking (`ldd` should say "not a
  dynamic executable").

## 8. Testing

- Unit: cache layout/atomic write, tree parsing, filter logic, keymap table.
- Component: ratatui `TestBackend` + buffer assertions for popup, panes,
  filter mode transitions; `insta` snapshots for the three-pane layout.
- GitHub client: `wiremock` against recorded API payloads; 304/ETag and
  rate-limit paths covered.
- Editor suspend/resume: integration test behind a flag (needs a TTY),
  otherwise smoke-tested manually.

## 9. Milestones

1. **Skeleton** — terminal lifecycle, component trait, mode enum, event loop.
2. **Search popup** — GitHub search + debounce + focus switching.
3. **Browser** — three panes, tree fetch, navigation, live filter.
4. **Preview** — blob fetch, cache, syntect highlighting.
5. **Editor** — suspend/resume + spawn.
6. **Cache hardening** — ETag revalidation, eviction, orphan sweep.
7. **Release pipeline** — docker, compose services, gh release, CI.
