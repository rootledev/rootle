# 0008 — Remote-provider hardening (transport, errors, restart)

Status: implemented (v0.3.3). Deferred slices in §6 remain open by
design.

## 0. Context

An external design review assessed the stdio protocol against a
remote, production-grade backend profile — a provider that crosses a
corporate network, is metered by a rate limiter, holds an expiring
credential, and can hang. Its root finding: the transport was
designed for a provider that is local, fast, free, trusted, and always
available — `fs_provider.py`, the only backend ever exercised. Every
blocking finding is that assumption surfacing:

- **B1**: one slow reply wedges the provider — `read_line` blocks
  under the io mutex with no timeout; advisory cancel can't interrupt
  the parked reader.
- **B2**: the v1.1 error taxonomy (`data.kind`) is specified but
  unwired — only `error.message` is read; the trait's `Result<_,
  String>` has no channel for structure; the lazy-context path
  swallows errors silently.
- **B3**: no server-initiated channel, no child-restart policy.
- **S1**: no debounce on lazy hit context (rate-limit budget burned on
  flyover hits). **S2**: requests serialize, no batch. **S3**:
  non-locatable hits render stale forever. **M1–M5** minors.

Every claim was verified against source before acceptance. The
sequencing below follows the review's (survivability, not effort).

**Tokio: explicitly rejected.** Cancellation semantics would be
identical (cancelling our wait, never the child's work — generation
counters + advisory cancel already do this), and every finding solves
with std primitives. Revisit if S2 grows into bounded structured
concurrency (`JoinSet`/`Semaphore`) or the event loop goes async.

**No handshake bump.** Everything here is host-side (timeout, restart)
or additive-and-ignorable on the wire (`data.kind`, `truncated`).
The handshake stays `protocol: 1`; the doc earns a v1.2 section. A
future server-initiated channel rides a capability flag in
`initialize` (`notifications: true`) under the reader-tolerance rule —
this commitment is locked now so B3's later slice needs no negotiation
design.

## 1. B1 — reader-thread transport (v1.2)

Replace the synchronous request loop with the standard LSP-client
shape:

- A **reader thread** owns the child's stdout, reads lines, and
  dispatches: replies go to a per-id oneshot/slot, anything id-less or
  id-unknown is discarded (and, later, routed to a notification
  handler — B3's seam).
- `request()` = write under the stdin lock (unchanged), then
  `recv_timeout` on the reply slot. Expiry → the request fails with a
  `timeout`-kinded error (§2), the io state stays consistent, the
  child stays alive. A late reply is discarded by id-matching.
- EOF (child death) closes the channel → **every pending request fails
  immediately** with a `provider`-kinded "child exited" error instead
  of hanging — half of B3's detection for free.
- `current_id` / `advise_cancel` keep working unchanged (cancel stays
  deduplication; now recovery exists too).
- Config: `[provider] timeout_ms` (default 30_000). The initialize
  handshake uses the same bound — a provider that hangs on startup
  fails into the github fallback with a clear message instead of
  blocking launch.

Rejected: OS-level `poll` on the pipe fd (musl-fragile, no dispatch
seam); tokio (§0).

## 2. B2 — structured errors through the trait

- New `ProviderError { kind: ErrorKind, message: String,
  retry_after: Option<Duration> }`; `ErrorKind` is an open enum
  (`Auth | RateLimited | NotFound | Network | Timeout | Provider |
  Other`) mirroring the v1.1 wire values. `type Result<T> =
  result::Result<T, ProviderError>` replaces `Result<T, String>` on
  every trait method, impl (github, stdio, offline), and call site.
- stdio parses `error.data.kind` + `retry_after_s`; unknown/absent
  kinds map to `Other` — wire-compatible with v1.1 providers,
  fs_provider untouched.
- github maps HTTP statuses: 401/403 → `Auth`, 429 (+Retry-After) →
  `RateLimited`, 404 → `NotFound`, reqwest connect/timeout errors →
  `Network`/`Timeout`. Today these are bare strings; the mapping is
  mechanical.
- Rendering: `auth` → persistent status with the recovery hint
  ("auth failed — refresh provider credentials"); `rate_limited` →
  "throttled, retry in Ns" status; everything else keeps today's
  transient toast. The lazy hit-context path stops swallowing:
  `Auth`/`RateLimited` errors surface a status line; other kinds stay
  quiet (bare path, retry on revisit).

## 3. S1 — cursor-rest debounce

`LoadHitContext` dispatch gets a 200 ms cursor-rest debounce: each
selection change bumps `context_gen` and spawns a timer thread that
sleeps, then dispatches only if its generation is still current.
Holding j through 25 hits costs one provider call (the resting one)
instead of 25 requests + 24 advisory cancels. `advise_cancel` on
supersession stays for the post-dispatch case.

## 4. S3-lite + minors

- **S3**: a fetched-but-unlocatable hit flips from `stale` to a
  distinct `unlocatable` state (chip + tooltip wording: "match text
  not found in blob — non-literal or moved") instead of rendering
  stale forever. `spawn_hit_context` emits a `HitContextMissing` event
  instead of returning silently.
- **M1**: comment at `code_query()` marking the emitted qualifier
  strings (`path:`, `repo:`, `org:`, `extension:`) as protocol surface
  — adapter authors translate them; changes need a protocol-doc note.
- **M2**: `[provider] stderr = "inherit"` (default `"null"`) pipes the
  child's stderr through for adapter debugging.
- **M3**: optional `truncated: bool` on `search/code` replies
  (additive; absent = false). The view shows a "results clipped by
  provider" note when set; `HIT_CAP` client-side clipping gets the
  same note so complete and clipped sets are distinguishable.

## 5. B3 — child-restart policy

On EOF or repeated transport failure, the provider respawns the child
with bounded backoff (e.g. 1s → 2s → 5s → 30s cap), re-runs
`initialize`, and only then fails requests. Restart events surface as
a status line ("provider restarted"). Policy knobs stay internal
constants until a real adapter asks. The server-initiated notification
channel is NOT in this slice — it lands (if open question 2 of the
review says mid-session auth matters) as an `initialize`-negotiated
capability, per §0.

## 6. Deferred (recorded, not re-litigated)

S2 batch method (`repo/blobs`) or id-keyed concurrency — the §1
transport makes both possible; add when an adapter demonstrates need.
B3 channel (§5 trigger). M4 stdio-side disk caching (adapter-side for
now; the GitHub provider's store is the template). M5 ref parameter
(ceiling, not regression). Pagination (deferred by agreement in the
review).

## 7. Milestones

One PR per milestone group, merged in order:

1. **B1 transport** — reader thread, `recv_timeout`, timeout config,
   handshake bound; fake-provider unit tests (slow, hung, die-mid-request).
2. **B2 errors** — `ProviderError` refactor, `data.kind` parsing,
   github status mapping, auth/rate_limited rendering, lazy-context
   surfacing.
3. **S1 + S3 + minors** — debounce, unlocatable chip, M1/M2/M3.
4. **B3 restart** — respawn with backoff + status surfacing.
5. **Release** — protocol doc v1.2 section, version bump (0.3.3),
   plan status flips.

## 8. Verification

- Unit: timeout expiry fails the request and the next request
  succeeds (hung fake provider); child death mid-request fails all
  pending immediately; `data.kind` parsing (each kind + unknown +
  absent); debounce collapses N selection changes into one dispatch;
  unlocatable event sets the new chip state; `truncated` parse.
- Render: auth status hint; rate_limited status with retry seconds;
  unlocatable chip; clipped-results note.
- e2e (fs provider): unchanged behavior end-to-end (the whole
  existing suite is the regression net); a scripted slow provider
  (sleeps 60s) proves the UI stays responsive and the call fails with
  the timeout message; a die-on-second-call provider proves restart
  + recovery.
- Docker gates + demo artifacts per the PR skill.
