# Changelog

User-visible changes per release. Protocol archaeology lives in
`plans/`; this file is for "what's new for me".

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

[0.8.2]: https://github.com/rootledev/rootle/releases/tag/v0.8.2
[0.8.1]: https://github.com/rootledev/rootle/releases/tag/v0.8.1
[0.8.0]: https://github.com/rootledev/rootle/releases/tag/v0.8.0
[0.7.1]: https://github.com/rootledev/rootle/releases/tag/v0.7.1
[0.7.0]: https://github.com/rootledev/rootle/releases/tag/v0.7.0
[0.6.0]: https://github.com/rootledev/rootle/releases/tag/v0.6.0
