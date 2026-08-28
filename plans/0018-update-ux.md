# 0018 — Update UX: the researched plan

Status: **done (2026-08-28)** — M1 stage UI on `rootle update`
(wiremock-asserted step sequence + changelog link), M2 once-a-day
toast via a `shown_at` stamp + CI/`TERM=dumb`/non-TTY gates, M3
quit-time relaunch line; shipped in 0.8.3.

## Research: what the good ones do

- **oh-my-pi (omp)**: one line at startup when a newer release exists,
  naming the command (`omp update`); the update itself is a series of
  labeled steps with a spinner and a "restart to apply" note (their
  release notes even track Windows self-update edge cases — swap
  semantics matter). Never blocks startup; never nags twice a day.
- **herdr**: background version check; a quiet banner line; `herdr
  update` replaces the binary in place and (for its server mode) hands
  sessions off live. The takeaway for rootle: the notice is ambient,
  the command is one word, the progress is visible.
- **update-informer (the canonical Rust crate)**: cache the check
  result on disk with a TTL; check at EXIT or startup off-thread;
  auto-silence in CI (`CI=true`) and offline. `gh upgrade` and `uv
  self update`: step-labeled output with spinners, a summary line with
  old → new, and a link to the release notes.

The pattern across all four: **ambient notice, one-word command,
visible progress, honest channels.**

## Where 0017 landed vs that

| Pattern | 0017 shipped | Gap |
|---|---|---|
| Background cached check | 24h cache, one startup call, silent offline | The status toast fires on **every launch** while an update exists — the cache gates the network call, not the nag |
| Notice names the command | status line + persistent `↑ vX.Y.Z` chip | fine |
| One-word update | `rootle update` | fine |
| Visible progress | none — one line at the end | the eye-candy gap (owner ask) |
| Channel honesty | brew/cargo/mise get their own command | fine |
| Restart semantics | "takes effect on next launch" (update output) | no in-TUI trace after a completed update |
| CI silence | offline apps skip | no `CI=true` skip |
| Release notes link | changelog exists; not linked from the update output or the chip flow | link it |

## Milestones

### M1 — Update gets the manager's stage UI (the eye candy)

`rootle update` drives `provider::ui::Ui` — the exact component the
provider install uses: step/done rows, a spinner per download,
"Verified sha256 ok", the summary with a timer:

```
Updating rootle
 ✓ Resolved   v0.9.0
 ● Downloading rootle-0.9.0-x86_64-unknown-linux-musl.tar.gz…
 ✓ Verified   sha256 ok
 ✓ Extracted  rootle
 ✓ Swapped    ~/.local/bin/rootle
 ✓ Updated 0.8.0 → 0.9.0 in 2.3s
 ▸ takes effect on next launch · what's new: rootle.dev/CHANGELOG.md#v090
```

- `update_inner` takes a `&Ui` (or returns staged progress events);
  the wiremock test asserts the step sequence on stderr.
- The changelog link uses the keepachangelog anchor.

### M2 — The notice stops nagging

- The status-line toast shows **once per version per 24h** (the cache
  file gains `shown_at`); the `↑` chip persists silently after that.
- Skip entirely when `CI=true`, `TERM=dumb`, or stdout isn't a TTY
  (update-informer's rules) on top of the existing offline +
  `[update] check` gates.

### M3 — Restart trace

After a successful update from inside a running rootle (the notice
flow tells you to run it in a shell — updating a live TUI's own binary
mid-session is fine on unix, the process keeps its inode): when the
chip's version ≤ current, the chip is gone (already true). No in-TUI
self-restart — deliberate: a TUI that re-execs itself loses the user's
state; the exit line after `q` following an update says "v0.9.0
installed — relaunch for it" when the on-disk binary is newer than the
running one. (Cheap: compare once at exit.)

## Decisions

- No auto-update, ever. No rollback UI (the release tarballs stay
  downloadable; `install.sh` with a version env can pin).
- The update flow stays unix-only (no Windows builds — unchanged).
- The notice never steals the status line while real status (loading,
  errors) is up — the chip is the persistent channel, the toast is
  the once-a-day one.

## Verification

- M1: wiremock loopback (0017's seam) asserting the step sequence +
  the summary line; a PTY run of `rootle update` against the staged
  fixture shows the frames for the PR.
- M2: a second app launch within 24h does not re-toast (cache unit
  test); CI=true produces zero output.
- M3: exit-line unit test on the version comparison only — no
  restart machinery to test, that's the point.
