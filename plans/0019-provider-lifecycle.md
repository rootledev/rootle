# 0019 — Provider lifecycle: `rootle update` sweeps, config declares

Status: **done (2026-08-28)** — M1 sweep shipped (#115), M2
declarative providers + consent flow shipped (#116); plus the
expanded-pane parity follow-through (#118) and the last-commit band
polish. Lock file rejected, revisit trigger noted in Decisions.

## Problem

Two gaps in the lifecycle story 0017/0018 left:

- `rootle update` updates the app and nothing else. Providers have
  their own subcommand tree — and brew/cargo/mise users get *nothing*
  from `rootle update`, even though providers live in rootle's data
  dir, outside the app's install channel.
- `rootle provider use <name>` writes a **materialized absolute argv**
  into `config.toml`. Sync that config to another machine and it is a
  time bomb: everything looks fine until the spawn fails and the app
  degrades to github. Intent ("use gitlab") and state (where the
  binary is) are conflated.

## M1 — `rootle update` sweeps providers

After the app flow — swap, channel guidance, or "is current", all of
them — the sweep runs over the manager's receipts:

- A new print-free core `Manager::sweep(dry_run) -> Vec<SweepOutcome>`
  (`Upgraded/Current/Pinned/Untracked/Failed`) beside
  `update`/`upgrade`; `rootle update` renders it with the manager's Ui
  grammar — per-provider rows, a counts summary with a timer.
- 0014's rules stay visible, not silent: pinned receipts are reported
  as skipped; plain-HTTP/`--path` sources are install-and-pin and
  never touched; only releases-API sources upgrade.
- Failures are isolated per provider (one dead forge doesn't abort the
  rest nor undo the app swap); any failure fails the command (exit 1)
  after everything was attempted.
- `--check` extends: the app line plus the stale list, no binary
  swaps.
- Empty store (no receipts): the section is skipped entirely.

## M2 — declarative providers: the config is the lock

`[provider] kind` grows a declared form:

```toml
[provider]
kind = "gitlab"        # receipt name, first-party alias, or owner/repo
tag = "v0.2.1"         # optional: pin exactly — omit to float
sha = "…"              # optional: verify the tarball against YOUR
                       # committed config, not just the forge sidecar
```

- Resolution order: receipt name → alias table (`gitlab` →
  `rootledev/rootle-gitlab`, `bitbucket` →
  `rootledev/rootle-bitbucket`) → literal `owner/repo` slug.
  Auto-fetch is restricted to releases-API slugs — never plain-HTTP
  URLs, never arbitrary argv.
- `provider use <name>` writes the declaration (`kind = name`, `tag`
  when pinned) instead of the absolute argv. Existing
  `stdio`+`command` configs keep working, unchanged precedence.
- Startup: declared + installed → spawn through the `current`
  symlink. Declared + missing → **consent popup** (y/N) carrying the
  trust line (`you are trusting rootledev/rootle-gitlab`); `y` runs a
  quiet install worker (status-line progress — no stderr stage UI
  inside the TUI), then hot-swaps the provider and the forge chip.
  `N`/Esc or failure → honest degraded mode: github fallback with a
  persistent status (`gitlab unavailable: <err> — browsing github`).
  Never silent, never a quiet channel swap.
- `install()` splits into `install_inner` (pure flow) + a UI wrapper —
  the `update_inner` pattern — so the TUI path gets the verified flow
  without writing to a raw terminal.

## Decisions

- **Consent, not magic.** A config file — possibly synced from
  somewhere you don't fully control — must not trigger silent
  download-and-execute. One deliberate keypress, mise-loud.
- **No lock file.** One active provider, an existing pin vocabulary,
  and the committed `tag`/`sha` already are the lock. Revisit when
  config grows a real multi-provider registry (`[providers.<name>]`
  tables) — that's when lock-file machinery pays rent.
- **Honest channels.** A failed declaration degrades loudly and the
  declared intent stays visible; retry is a keypress, not a shrug.
- The modeline `↑` notice stays app-only; provider staleness is
  `rootle update`'s to report, not a startup nag.

## Verification

- M1: loopback sweep test — stale upgraded, pinned skipped and
  reported, untracked reported untouched, one failing provider
  isolated; `--check` swaps nothing; the app+provider output sequence
  asserted.
- M2: resolution unit tests (receipt/alias/slug precedence, stdio
  untouched, `tag` honored, `sha` mismatch refuses); render snapshots
  of the consent popup (pending + declined status); e2e with
  `kind = "gitlab"` uninstalled: prompt appears, `N` → honest status,
  browsing continues on the fallback; the fs-provider PTY suite stays
  hermetic (the stdio path is unchanged).
