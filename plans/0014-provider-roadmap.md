# 0014 — Provider roadmap: backlog, reserved shapes, and the v2 question

Status: **not started** (consolidated provider backlog; each item
lists its state, rationale, and intended consumer)

This is the running register for everything provider-facing that has
been designed or promised but not built, plus the handover context a
fresh session needs to continue any of it.

## Current state (as of 2026-08-27)

- **rootle 0.6.0** (crates.io, 4-platform releases): protocol v1.3 —
  `$/partial` streaming search with inactivity deadlines, provider-
  declared modeline icons, per-item `line` anchors, legal path-only
  hits, `index.as_of` freshness, `file_search` capability split.
  Chrome: powerline modeline, bat-style gutters, fzf prompts,
  `[ui] border` / `[ui] nerd_font`.
- **rootle-gitlab 0.1.0**: ci/audit/live/release workflows; declares
  the gitlab icon.
- **rootle-bitbucket 0.1.0**: live-validated against bitbucket.org
  (CHANGE-2770 fixes, `/2.0/user/workspaces` discovery, `--workspace`
  bypass, parallel tree walk ~7s cold); publishes via tag; crates.io.
- **homebrew-tap**: 0.6.0 formula + styled cask, full matrix green.
- **rootle.dev**: providers section with sub-pages, syntax-
  highlighted docs, installbar with platform tag.
- **Enterprise counterpart**: the GHE-feedback team — they implement
  every accepted ask and report shapes before freeze. Their asks this
  roadmap honors.

## Backlog register

| # | Item | State | Notes |
|---|---|---|---|
| 1 | Richer `org/repos` items | **accepted, wire shape agreed** | Object entries `{name, description?, private?, archived?, pushed_at?}` alongside the string form (union-free: optional fields ride reader tolerance). Consumer: clone-wizard sort-by-pushed + grey archived. Additive v1.4. |
| 2 | `repo/clone_plan` | **designed, reserved** | `{url, args?: [str], env?: {str: str}}` — provider controls everything git-related (credential helper via `-c`, SSH-vs-HTTPS via url, shallow/partial via args, mirrors via url); rootle keeps destination + progress + per-repo failure. Implement with the clone wizard's spawn path; GHE team is the first consumer. |
| 3 | `repo/clone` (provider-performed) | **declined for v1.x** | First feature that writes outside the cache subtree, needs progress notifications and a deadline exemption. Answer to the GHE team stays: solve via `clone_plan`; revisit with `$/progress`. |
| 4 | `limit` (bounded compute) | **accepted, doc-only for now** | The anti-hammer contract: `search/code` params carry `"limit": N` (rootle's render budget, 500) — the provider stops scanning at ~N, sets `truncated`, never computes what the client would clip. Pagination-shaped input, streaming output, no cursor. Documented in the spec as advisory; adapters may honor early. Rootle sends it when the 0012 grammar work lands (one PR, ~5 lines). |
| 5 | `offset`/`next_offset` | **reserved, not consumed** | Cursor names stay blessed-but-frozen; only ever if the results view grows a load-more, which `limit` + narrowing makes unlikely. |
| 6 | `$/progress` (work-done progress) | **v2 question** | LSP's other half (WorkDoneProgress). Needed by repo/clone, cold-org enumeration, big tree walks. The reader-thread groundwork exists (v1.2); partial results took request-scoped `$/partial`, progress should take its own notification route like LSP. Decide with the first consumer, not before. |
| 7 | Full-tree progressive streaming | **not built** | Org enumeration is still one blocking call. The `$/partial` route exists; add when the GHE team's cold-start numbers justify it (they said cache-hard is acceptable). |
| 8 | Bitbucket private-workspace validation | **pending first workspace** | The account has zero workspaces; the private path (auth-gated org listing, private trees) is validated by construction only. Create one, push a repo, run the live workflow. |
| 9 | rootle-gitlab/bitbucket: full-file preview & symbol support | **follows 0012/0013** | `language:` translation (M1), `search/symbols` translation (0013): GitLab advanced search maps cleanly; Bitbucket stays file_find-only. |

## Decisions already made (don't relitigate)

- Streaming over pagination (v1.3, plans/0011). Offset names are
  reserved, never to be consumed without a load-more UX that needs
  them.
- Icons and names are provider-owned (handshake `icon`/`name`); rootle
  hardcodes only its own in-tree github. Config overrides exist for
  users, never for rootle's own guesses.
- Capability splits beat grammar translation when the backend is
  honest about absence (`code_search: false` > silent empty results).
- "Index-based, may be approximate" is said on-screen (`stale`,
  `located: false`, `index.as_of`) whenever rootle or a provider is
  guessing — GitHub's web UI doesn't give users this; it's ours to
  keep.
- No central provider registry in v1 of the manager; `--path` and
  arbitrary-host HTTP sources are supported deployment shapes, and
  `update`/`upgrade` track only releases-API sources (docs state it).

## Handover: how to pick any of this up fresh

- Repo layout and contracts: `AGENTS.md` (build gates, layout),
  `doc/house-style.md` (component behavior), `doc/provider-protocol.md`
  (the wire spec, v1.3), `plans/0011` (streaming design), this
  roadmap.
- Gates before any merge: host `cargo fmt && cargo clippy --all-targets
  && cargo test`, `cd e2e && uv run --locked python -m pytest`, then
  **both docker gates** (`docker compose run --build --rm test`,
  `docker compose run --build --rm e2e`) — docker's newer clippy
  catches things host clippy misses (it has, twice).
- PR conventions: `.agents/skills/rootle-pr/` — template + evidence
  (before/after PTY frames via the e2e harness; no vision model in
  this environment, so frames + behavior tests are the proof).
  Branches `feat|fix|chore/<slug>`, merge with `--merge --admin`.
- TUI debugging: `.agents/skills/rootle-tui-debug/` (PTY driver
  `e2e/tui.py`, pyte screens, hub PTY).
- Demo artifacts re-render via the `demo` workflow on src/demos/e2e
  changes (font family is `JetBrainsMono NFM` — the stock-font trap is
  recorded in `.agents/skills/rootle-demo-capture/`).
- Secrets in use: `CARGO_REGISTRY_TOKEN` (both crates), tap/site PATs,
  Bitbucket live token (Bearer; app-password shape would need
  username too — they don't mix).
- Environment: WSL2 host, no vision model, no Chromium daemon — verify
  with PTY frames, jsdom (site behavior), and wiremock (provider
  wire), not screenshots.
