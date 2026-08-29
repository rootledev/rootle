# 0022 — Fallback visibility: never quietly on github

Status: **done (2026-08-29)** — M1 sticky degraded, M2 health
prompt (r/g/e), M3 warning-tinted forge chip; lands with this flip. — owner feedback:
"silent fallback to github needs to be more visible and give the user
the option." Research below; plan follows.

## Problem

A misconfigured or crashed provider degrades rootle to github with a
single transient status line (`provider stdio failed (ENOENT); fell
back to github`) that any transient status can overwrite seconds
later. The user then browses github believing they're on gitlab.
0019 M2 made *one* fallback path honest (declared-but-missing →
consent popup + sticky degraded notice); the other four paths still
degrade quietly:

| Path | Today | 0019-covered? |
|---|---|---|
| declared + not installed | consent popup → y install / n github, sticky notice | ✓ |
| declared/stdio spawn fails at startup | one transient status | ✗ |
| malformed `kind` (typo, bad slug) | one transient status | ✗ |
| `kind` names a plain-HTTP tarball | one transient status | ✗ |
| no provider data dir | one transient status | ✗ |
| spawn dies **mid-session** | child respawn notices ride the status line once | ✗ (restart loop is invisible) |

## Research findings

- The pieces exist: `BuildOutcome::Warn(String)` funnels all of these;
  the `degraded` sticky slot (0019 M2) renders persistently and only
  yields to transient statuses; the consent popup is the working
  choice surface.
- The failure classes split cleanly: **provisioning** (not installed →
  install) vs **health** (installed but won't start / dies at runtime
  → retry/repair). One surface each.
- GitHub's own pattern (gh extension load failure) prints the error
  and stays on the page; mise prints a loud warning and continues.
  Nobody silently swaps backends — and rootle's own honest-channels
  principle (0018) already bans pretending.

## Milestones

### M1 — sticky degradation everywhere

`BuildOutcome::Warn` lands in the `degraded` slot (not transient
`status`), with the provider + cause named: `gitlab failed to start
(ENOENT) — browsing github`. Mid-session respawn-death (N consecutive
failures, N small — the restart machinery already counts) also lands
there once, not per attempt.

### M2 — the health prompt (the user's "option")

Spawn failure at startup raises the consent popup in a **health
variant**: `gitlab failed to start (ENOENT)` with three keys —
`r` retry (respawn once), `g` browse github (sticky notice stays),
`e` edit config (opens the config file in the editor; Esc returns).
Malformed kinds get `g`/`e` only. Mid-session death raises nothing
interactive — the sticky notice is the surface; the restart machinery
already respawns with backoff.

### M3 — the chip says so

The modeline forge chip tints `warning` while degraded (running on a
fallback) — a persistent ambient marker until the declaration
succeeds or the session ends. New semantic role not needed (warning
exists); the chip already re-renders per frame.

## Decisions

- No new mode; the consent popup carries both variants (provisioning,
  health) — one component, one key grammar.
- No auto-repair beyond a single `r` — retry loops belong to the
  restart machinery's backoff, not the UI.
- Mid-session respawn-death never prompts (interrupting a session for
  infrastructure is worse than the sticky notice).

## Verification

- M1: render tests — every `BuildOutcome::Warn` path leaves the
  degraded line visible after other statuses clear.
- M2: e2e (fs fixture with a bad command → health prompt; `r` respawns,
  `g` degrades sticky, `e` opens config in `$VISUAL`); malformed kind
  e2e (`g`/`e` only).
- M3: render test — degraded chip tints warning; clears when a
  declared provider spawns successfully.
