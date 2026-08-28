# 0016 — Product direction: the browser for repos you don't have

Status: **M1 done (2026-08-27)** — mock reviewed + approved, protocol v1.5 landed (refs/log/blame/blob_at), all four providers implement (bitbucket blame:false honestly), conformance FC-090..099 gate it; from the external
||||||| 9c5143e
Status: **accepted as direction (2026-08-27)** — from the external
product review of the 0.7.0 state. The positioning work (site,
provider comms) shipped the same day (rootledev.github.io#12/#13);
this plan sequences the product investments it names.

## The thesis

The review's sharpest cut: rootle is not "a GitHub TUI" —

> **rootle is the terminal-native code browser for all the
> repositories you don't have checked out.**

The provider seam (github in-tree, gitlab/bitbucket installable,
arbitrary/internal forges one stdio adapter away) is the most
defensible thing about the product, and 0.7.0 made it real:
conformance-gated adapters, a checksum-verified manager, multi-forge
search. The site now leads with that. What follows is code-browsing
depth, not breadth.

## Non-goals (the review was emphatic; don't relitigate)

- **No gh-dash.** No notifications/issues/PR-dashboard. PR/MR support
  arrives only as "PR as another lens over source" (M3 below).
- **No generic git frontend.** Revision awareness is for browsing,
  not staging/committing/pushing.
- **No AI features.** Speed, deterministic navigation, composability.
- **No SaaS-gloss homepage.** The sparse terminal-ish presentation
  fits; keep it.
- The Miller-column interaction is the identity — keep it.

## Milestones

### M1 — Revision awareness (next major browsing investment)

The gap: trees/blobs/search all pin the default branch today
(`repo/tree` params carry only `{"repo"}`; the reply's `branch` is
informative, not selectable). "This only happens on release/2.7"
should make someone reach for rootle. Three sub-milestones, each
shippable alone.

**M1a — branches & tags (v1.5 candidate, additive).**
Wire:
- `repo/refs` — `{"repo"}` →
  `{"branches":[{"name","sha","default"?}], "tags":[{"name","sha"}]}`.
  GitHub `/branches` + `/git/refs/tags`, GitLab `repository/branches` +
  `tags`, Bitbucket `refs/branches` all map directly.
- `ref?` param on `repo/tree` (the only content call that needs it —
  `repo/blob` is sha-keyed, `repo/web_url` already takes `branch`).
- Capability `refs` (default false = default-branch only, said
  honestly where the switcher would be).
UX:
- CLI: `rootle owner/repo@ref` (ref = everything after the first `@`;
  slashes legal: `release/2.7`).
- The repo's modeline crumb reads `repo @ branch` when off-default.
- `␣ b` opens the refs popup — the scope-radio pattern (live follow,
  Enter commits, Esc reverts, `/` filters): branches first, tags
  dimmed below. Switching refetches the tree; the status line names
  the switch.
- Search honesty: GitHub REST code search only indexes the default
  branch — grep off-default shows a `search: <default> only` chip
  there; GitLab translates (`ref` param); fs/bitbucket walk the
  switched tree, so they search the revision you're looking at.
Cache: ref→sha is the only mutable mapping and already exists
ETag-revalidated in the github provider (`index/refs/…`); trees/blobs
stay content-keyed and immutable. Verify this by construction, not by
hope — a conformance case pins "tree at ref A ≠ tree at ref B ⇒
different shas, both cached".

**M1b — file history (`repo/log`).**
Wire: `{"repo","path"?,"ref"?,"limit"?}` →
`{"items":[{"sha","subject","author","date"}], "truncated"?}` —
`limit` rides the bounded-compute decision (0014 #4). All three
backends have it (GitHub `/commits?path=&sha=`, GitLab
`repository/commits?path=&ref_name=`, Bitbucket `commits?path=`).
UX: `␣ h` on a previewed file turns the preview pane into the commit
list (subject · author · relative date, tig-shaped); Enter opens the
file AT that commit in the preview (read-only; the adapter resolves
commit → tree → blob sha), Esc returns to the present. Yanking from a
historical view anchors the URL to the commit sha — a permalink that
never rots. v1 has no diffs (see non-goals: not a git frontend).

**M1c — blame (revived 2026-08-27, owner call: "too good not to
have").** UX decision, after comparing VSCode/Zed's always-on margin
annotations: rootle does **fugitive-style run margins as a lens**,
not a persistent per-line gutter — a margin that is always on fights
the pane's width and adds noise to every line. `␣ p b` toggles the
blame lens: the preview gains a left margin where each commit's run
carries `sha + author` on its first line and a dim dot leader on
continuations; the line cursor walks it; Enter on a line opens the
history lens AT that commit (the two lenses compose — no second way
to inspect a commit). Wire: `repo/blame {"repo","path","ref"?}` →
`{"ranges":[{"start_line","end_line","sha","author","date"}]}` (line
ranges, not per-line — 4× smaller replies, runs are the render unit
anyway). Backends: GitLab `repository/files/:path/blame` (REST ✓),
GitHub GraphQL `blame` (the provider's one GraphQL call — contained),
fs via `git blame` when the served dir is a worktree, Bitbucket has
NO blame API → capability `blame: false`, honest chip. Three of four
backends clears 0013's one-provider bar.

**The preview submode (M1's shell, mocked 2026-08-27).** The pane was
accumulating keys (J/K line cursor, ␣ / find, history, blame) — they
now live in one place: `␣ p` focuses the preview AND zooms it to the
full content row (the tmux `prefix z` model — focusing a pane without
giving it width is a half-gesture, and the blame margin needs the
columns). Inside: j/k walk lines (lowercase twins of Browse-mode
J/K — same cursor, two speeds), `/` is the existing find-in-file
session, `h` history, `b` blame, Enter is the editor handoff (or the
line's commit while blaming), Esc unwinds. Every lens is
`/` filterable per the house rule; Esc ladders (filter → lens →
submode → browse). Browse-mode J/K stays — quick peeks and yank
anchors don't pay the mode switch. Rejected: a separate `␣ z`
pane-generic zoom — nothing but the preview benefits today; the zoom
machinery is written pane-shaped so `z` can come when a second pane
earns it.

Conformance: forge-conformance gains FC-09x cases (refs listing
shape, tree-at-ref sha discipline, log shape + limit) with the wire
spec — the suite is where these land first.

### M2 — Provider onboarding polish

Mostly landed 2026-08-27 (manager-first docs, 30-second install,
trust signals, arbitrary-host installs in 0.7.0). Remaining:

- In-app `:provider` flow — browse/install/switch without leaving the
  TUI (the manager is CLI-only today).
- First-run hint when a search fails for capability reasons
  (`code_search: false` → "install X for content search" guidance).

### M3 — PR/MR support: **declined by the owner (2026-08-27)**

"I don't want to expand the tool to PRs/issues — PRs/issues are for
gh-dash or the web side." rootle stays focused on code. The review's
caution is adopted as a boundary: not even the read-only lens. If a
provider-shaped argument ever changes this (e.g. reviewing a PR's
*source* without a checkout turns out to be the same browsing
problem), it comes back as a fresh plan — this paragraph is the
graveyard marker, not a backlog item.

### M4 — Enterprise/self-hosted story

The GHE feedback loop + conformance suite are the foundation; what's
missing is packaging: a "your company forge" walkthrough (GHE
in-tree? github provider with a custom base URL is the obvious v1),
org-wide config snippets, and the manager's plain-HTTP install story
documented as the air-gapped path.

### M5 — Demo tells a workflow story

The site demo heading changed 2026-08-27 ("watch the workflow — then
pick a palette"); the tape should match: find repo → browse source →
grep with grammar → expand a hit to the full file → facet chips →
yank permalink. Palette picker stays as the secondary control. Edit
`demos/demo.tape`; the demo workflow re-renders (gotchas in
`.agents/skills/rootle-demo-capture/`).

## Discoverability (done 2026-08-27, kept here as the record)

- Repo homepage metadata → rootle.dev (was the github.io path).
- Title/meta carry "rootle — terminal source browser" everywhere the
  name appears (site, crates.io description on next release, repo
  description stays as-is — it already reads well).
- No rename: rootle.ai is a different space; consistent phrasing is
  the fix.

## Verification (per milestone, when built)

- M1: conformance cases for `ref` round-trips; e2e against the fs
  provider serving two branches; render tests for the switcher.
- M3: wiremock fixtures for pr/files/diff on both adapters; frames of
  the changed-files column → diff flow.
- M5: the demo workflow re-render is the proof.
