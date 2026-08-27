# 0016 — Product direction: the browser for repos you don't have

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
should make someone reach for rootle.

- `rootle owner/repo@branch` CLI grammar + an in-app revision
  switcher (branches/tags popup on the repo pane).
- Protocol: additive `ref?` param on `repo/tree`, `repo/blob`,
  `search/code` scope, `repo/web_url`; replies already carry `branch`.
  Refs listing: `repo/refs` (branches + tags). Conformance cases in
  forge-conformance; adapters translate (GitLab `ref=`, Bitbucket
  `at=`, GitHub `ref`).
- Then: commits view (`repo/log`), file history, and blame
  (`repo/blame` — the heaviest; GitHub REST has no blame, GraphQL
  does — decide backend-by-backend, capability-gated, honest when
  absent).
- Design gate: how refs interact with the content-keyed cache (sha
  addressing makes this safe by construction — verify, don't assume).

### M2 — Provider onboarding polish

Mostly landed 2026-08-27 (manager-first docs, 30-second install,
trust signals, arbitrary-host installs in 0.7.0). Remaining:

- In-app `:provider` flow — browse/install/switch without leaving the
  TUI (the manager is CLI-only today).
- First-run hint when a search fails for capability reasons
  (`code_search: false` → "install X for content search" guidance).

### M3 — PR/MR as a lens over source

Read-only, source-centric: select PR → changed files as a miller
column → diff view → surrounding file context → open locally / yank
permalink. Wire: `pr/list`, `pr/files`, `pr/diff` shapes designed
when the first provider implements it (gitlab's MRs API and GitHub's
pulls/files both map). Explicitly NOT: reviews, comments threads,
checkout/push/update actions.

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
