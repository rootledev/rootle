# rootle — Plans

A modal ratatui TUI for browsing remote source-control systems with a
yazi-like flow — the terminal source browser for repos you don't have
checked out. No local clone required: GitHub ships in-tree, everything
else is a stdio provider speaking NDJSON-RPC (doc/provider-protocol.md),
with a content-addressable disk cache under `~/.cache/rootle`.

Plans are numbered, oldest first. Status lives in each plan's header;
per-release notes are on the GitHub releases page:

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
- [0006-provider-v1.1](plans/0006-provider-v1.1.md) — located/stale
  hits, advisory cancel, lazy context fetch.
- [0007-preview-theming-and-find](plans/0007-preview-theming-and-find.md)
  — themed preview, line cursor, find-in-file.
- [0008-remote-provider-hardening](plans/0008-remote-provider-hardening.md)
  — transport v1.2: reader thread, timeouts, restart obligations,
  error taxonomy.
- [0009-gitlab-provider](plans/0009-gitlab-provider.md) — the first
  out-of-tree provider.
- [0010-provider-manager](plans/0010-provider-manager.md) — install /
  update / upgrade / pin / use for provider binaries.
- [0011-progressive-search](plans/0011-progressive-search.md) — v1.3
  `$/partial` streaming with inactivity deadlines.
- [0012-search-ux-parity](plans/0012-search-ux-parity.md) — query
  grammar, full-file preview, facets. Done.
- [0013-symbol-search-gate](plans/0013-symbol-search-gate.md) — symbol
  search; the tree-sitter spike passed, implementation next.
- [0014-provider-roadmap](plans/0014-provider-roadmap.md) — the running
  provider backlog register (limit, org/repos metadata, manager scope,
  handler splits, conformance suite — all landed).
- [0015-forge-conformance](plans/0015-forge-conformance.md) — the
  canonical numbered gate, live at rootledev/forge-conformance.
- [0016-product-direction](plans/0016-product-direction.md) — the
  current direction: revision awareness (refs, history, blame), the
  preview submode, what rootle will NOT become.
- [0024-crate-split](plans/0024-crate-split.md) — six-crate virtual
  workspace and dependency-ordered publishing. **Implemented.**
- [0025-typed-identity](plans/0025-typed-identity.md) — provider
  identities, domain request clocks and entity-scoped marks. **Implemented.**
- [0026-unified-components](plans/0026-unified-components.md) — shared
  list/filter/viewport mechanics and table-driven keys. **Implemented.**
- [0027-provider-protocol-tla](plans/0027-provider-protocol-tla.md) —
  bounded safety/temporal checks, four kept faults and the Rust bridge.
  **Implemented.**
- [0028-commit-viewer](plans/0028-commit-viewer.md) — `repo/commit`
  (v1.6), checked deltas and the detail → file-delta chain. **Implemented.**
- [0029-polish-release-integration](plans/0029-polish-release-integration.md)
  — final integration, evidence, green CI and verified v0.10.0 release.
  **Released as v0.10.0; published evidence lives here.**
- [0030-session-diagnostics](plans/0030-session-diagnostics.md) —
  private bounded JSONL sessions, UI/service observations and diagnostic
  controls adapted from strop. **Implemented and verified; unreleased.**


Source comments referencing `plans/0005` mean this file.

Section references in source comments (`PLAN.md §N`) point at
`plans/0001-v0.1.md` unless a comment names a specific file.
