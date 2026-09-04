# Changelog

User-visible changes per release. Protocol archaeology lives in
`plans/`; this file is for "what's new for me".

## [0.9.1] — 2026-09-04

### Fixed

- A failed blob fetch (binary file, dead endpoint) no longer regresses
  to a permanent "loading…" placeholder on re-select — the error is
  cached and re-shown, and ␣ r retries. Stale failures no longer
  clobber the file you moved to. (Found by the headless breaker.)
- The "blame…" modeline transient clears when the blame lands.
- Popups enforce a size floor: at tiny viewports the search popup no
  longer guillotines its results box or leaks cells past the border.

## [0.9.0] — 2026-09-04

### Added

- `--headless SCRIPT` (plans/0023): a scripted, deterministic driver
  for the TUI — `keys`/`settle`/`frame`/`state` steps in, cell-grid
  frames and state JSON out. No terminal, no PTY, no timing
  heuristics; the same input path the TUI uses. For tests, reviews,
  and agent-driven stress runs (`-` reads the script from stdin,
  `ROOTLE_HEADLESS_COLS/ROWS` size the viewport).
- macOS behavioral CI: the full e2e suite (headless + PTY) now runs
  natively on a macOS runner every push — not just release builds.
- Verified releases: every release tarball now carries GitHub build
  provenance, verifiable with
  `gh attestation verify <tarball> -R rootledev/rootle`. The release
  smoke also proves each shipped binary renders (headless), not just
  links.

### Fixed

- XDG base dirs are honored on macOS too: config/state/cache/data
  live at `~/.config|~/.local/state|~/.cache|~/.local/share` as the
  docs promise — not `~/Library/…` (caught by the new macOS CI job;
  settings write-back silently landed there before).

## [0.8.7] — 2026-08-29

### Added

- Mid-session provider death is honest too: a restart-failure streak
  surfaces once — sticky notice, warning-tinted forge chip — until the
  provider recovers (instead of transient per-request errors).

## [0.8.6] — 2026-08-29

### Added

- Fallbacks are never quiet: a degraded provider (spawn failure,
  bad config, tarball-kind, missing data dir) leaves a sticky modeline
  notice naming the provider and cause — transient statuses overlay
  but never erase it, and the forge chip tints warning until the
  declaration succeeds.
- The provider health prompt: when the configured provider won't
  start, rootle asks — `r` retry once, `g` browse github (sticky
  notice stays), `e` edit the config in your editor. Kinds that can't
  be retried (plain-HTTP tarball refs) offer `g`/`e` only.

## [0.8.5] — 2026-08-29

### Added

- Search-pane `y` works in the repo search popup too — `y` now yanks
  the selected repo or org URL; every context honors or names its
  keys (the keymap tables gained a conformance test: every advertised
  row resolves to a real action).

### Changed

- Architecture pass (0021): `app/mod.rs` 1929 → 606 lines; workers,
  browser, preview, global-search and theme split along their seams
  (workers/search/lenses/lifecycle, browser/lenses/blobs,
  preview/lens/motion, global_search/pane, theme/palettes) — pure
  moves, zero behavior change.
- rootle-gitlab answers requests concurrently (v1.3): slow requests
  (tree fetches, blame walks) no longer stall previews behind them;
  transcripts stay id-tagged and line-atomic. rootle-gitlab ≥ this
  release carries it.

## [0.8.4] — 2026-08-28

### Added

- `rootle update` sweeps providers too: every managed, unpinned,
  releases-tracked provider upgrades through the same verified flow,
  pinned and install-and-pin sources report untouched, and one dead
  forge fails only itself. brew/cargo/mise installs get the sweep as
  well — providers live outside the app's channel.
- Declarative providers: `[provider] kind = "gitlab"` (or any receipt
  name / `owner/repo` slug) resolves at startup; a missing one asks
  (`y install · n browse github instead`), installs checksum-verified,
  and hot-swaps. Optional `tag`/`sha` pins lock the build. `provider
  use` writes the declaration — no more absolute paths in synced
  configs.
- The preview header band dresses every file with its last commit
  (`sha · subject · author · date`, ambient and memoized) — not just
  at-commit views.
- The grep/find expanded pane speaks the preview-submode grammar: `y`
  yanks the cursor-anchored URL, `:N` jumps by line, `b` runs the
  blame lens. Query matches chip in the pane too, boundary-aware.
- Find's current match is bold+underlined — `n`/`N` reads at a glance
  on any palette.

### Fixed

- Scoped greps no longer show a quiet zero when GitHub's code index
  omits the repo (young/low-activity repos): rootle falls back to a
  local grep over the default-branch tarball — real line numbers,
  real blob shas.
- GitHub blame works again: upstream removed the GraphQL `Blob.blame`
  field; blame is now a bounded commits-walk over REST (parallel
  detail fetches, session cache).
- Match chips survive comment-span boundaries: a needle split across
  syntax spans chips instead of vanishing.

## [0.8.3] — 2026-08-28

### Added

- `rootle update` shows staged progress — resolved, downloading
  (spinner), verified, extracted, swapped, a timed `0.8.2 → 0.8.3`
  summary, and a link to the changelog anchor for what's new.
- Quitting a session whose binary was updated in a shell prints
  `vX.Y.Z installed — relaunch for it` after the terminal restores.

### Fixed

- The update toast nags once per version per 24h instead of on every
  launch — the `↑ vX.Y.Z` modeline chip remains the persistent notice,
  and it never replaces a busy status line. The startup check is
  skipped entirely in CI, on dumb terminals, and when stdout isn't a
  terminal.

## [0.8.2] — 2026-08-28
### Added

- Preview header band: the full path rides every file preview;
  at-commit it dresses with `sha · subject · author · date`.
- vim visual-lines in file panes (`v` in the `␣ p` submode and a search
  hit's expanded pane): motions extend the selection, `Y` copies it
  (the cursor line by default), `y` yanks a `#L3-L7` range URL.
- The search query field restyles grammar tokens (qualifier
  key/value, quoted literals, negation markers take syntax colors).

### Fixed

- The leader layer raises over the preview submode.

## [0.8.1] — 2026-08-28

### Fixed

- Open-at-commit views keep syntax highlighting (highlighting reads
  the real path; the `@ sha` marker stays title-only) and the restore
  no longer keeps the marker title.
- fs reference adapter: trees at a ref list directories (git ls-tree
  needs `-t`) — the demo render caught undrillable columns.

## [0.8.0] — 2026-08-28

### Added

- Revision awareness (protocol v1.5): `␣ b` switches branches/tags
  (`rootle owner/repo@ref` too), `␣ p h` file history with
  open-at-commit, `␣ p b` blame lens with run margins, permalinks
  anchored to commit shas (`y` in the history lens).
- The preview submode (`␣ p`): the pane focuses and zooms; vim
  vertical motions (counts, `gg`/`G`, `ctrl-d/u/f/b`, `{`/`}`, `%`,
  `zt/zz/zb`, `:<line>`); find-in-file returns to the submode;
  `n`/`N` cycle matches there.
- Modeline: state-only with a `? keys` affordance; transient modes get
  a one-line hint strip glued above it. Chip separator is a pipe by
  default — `[ui] separator = "caret"` restores `❯`; Nerd Font still
  draws powerline arrows. `[ui]` rows are live-editable in `:settings`.
- `rootle update` — self-update for install.sh/tarball installs
  (checksum-verified, atomic); brew/cargo/mise installs get their
  channel's command instead. Modeline notice `↑ vX.Y.Z` when a newer
  release exists (24h-cached, one startup call, silent offline;
  `[update] check = false` disables).

## [0.7.1] — 2026-08-27

### Added

- Search results render as decorated boxes: the filename rides the top
  rule, the match badge closes it right, `│` gutters between the
  rails; follows `[ui] border`.

## [0.7.0] — 2026-08-27

### Added

- Search grammar (protocol v1.2-UX): quoted literals, `-`/`NOT`
  negation, `language:` qualifier; client-side subtraction with
  `filtered N` / `unfiltered:` honesty chips in the results title.
- Full-file preview: Enter on a search hit expands to the whole file
  at the match line; `/` finds inside it; Esc folds back.
- Facet chips: per-repo and per-language filters computed from the
  streaming result set — zero backend cost.
- Protocol v1.4: `limit` bounded compute on `search/code`; richer
  `org/repos` items (description/private/archived/pushed_at) — the
  clone wizard sorts by push date and greys archived repos.
- Provider manager: plain-HTTP artifact-host installs (checksum
  sidecar mandatory); `update`/`upgrade` track only releases-API
  sources — plain-HTTP and `--path` installs are install-and-pin.
- forge-conformance: the canonical provider gate (37 numbered cases),
  wired into rootle's, rootle-gitlab's, and rootle-bitbucket's CI.

### Fixed

- rootle-gitlab learned v1.3 streaming; rootle-bitbucket's
  branch-keyed tree cache stopped serving the first-fetched tree
  forever (both caught by the conformance suite).

## [0.6.0] — 2026-08-27

### Added

- Protocol v1.3: `$/partial` streaming search with inactivity
  deadlines, provider-declared modeline icons, per-hit line anchors,
  path-only hits, index freshness (`index.as_of`), `file_search`
  capability split.
- Chrome: powerline modeline (Nerd Font opt-in), bat-style gutters,
  fzf prompts, `[ui] border` / `[ui] nerd_font`.

[0.9.1]: https://github.com/rootledev/rootle/releases/tag/v0.9.1
[0.9.0]: https://github.com/rootledev/rootle/releases/tag/v0.9.0
[0.8.7]: https://github.com/rootledev/rootle/releases/tag/v0.8.7
[0.8.6]: https://github.com/rootledev/rootle/releases/tag/v0.8.6
[0.8.5]: https://github.com/rootledev/rootle/releases/tag/v0.8.5
[0.8.4]: https://github.com/rootledev/rootle/releases/tag/v0.8.4
[0.8.2]: https://github.com/rootledev/rootle/releases/tag/v0.8.2
[0.8.3]: https://github.com/rootledev/rootle/releases/tag/v0.8.3
[0.8.1]: https://github.com/rootledev/rootle/releases/tag/v0.8.1
[0.8.0]: https://github.com/rootledev/rootle/releases/tag/v0.8.0
[0.7.1]: https://github.com/rootledev/rootle/releases/tag/v0.7.1
[0.7.0]: https://github.com/rootledev/rootle/releases/tag/v0.7.0
[0.6.0]: https://github.com/rootledev/rootle/releases/tag/v0.6.0
