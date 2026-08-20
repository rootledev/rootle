# ghx — Plans

A modal ratatui TUI for browsing remote GitHub repos with a yazi-like
flow. No local clone required: the backend is the GitHub REST API, with
a content-addressable disk cache under `~/.cache/ghx`.

Plans are numbered per release, oldest first:

- [0001-v0.1](plans/0001-v0.1.md) — core browser: component library,
  miller panes, GitHub search/trees/blobs, cache, editor, CLI, release
  pipeline. **Shipped as v0.1.0.**
- [0002-v0.2](plans/0002-v0.2.md) — global search view (file find +
  grep) with Zed-style results. **Current.**
- [0003-v0.3](plans/0003-v0.3.md) — interaction layer: yank to
  clipboard, `?` keybinds popup, `:` command line, settings popup.
- [0004-v0.4](plans/0004-v0.4.md) — clone flow: VISUAL multi-select,
  `:clone` wizard (repos → destination → summary).
- [0005-provider-seam](plans/0005-provider-seam.md) — backends behind
  `trait Provider`; GitHub in-tree, external stdio providers via
  NDJSON-RPC (LSP model). Cross-cuts v0.2+.

Source comments referencing `plans/0005` mean this file.

Section references in source comments (`PLAN.md §N`) point at
`plans/0001-v0.1.md` unless a comment names a specific file.
