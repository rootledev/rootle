# 0027 — Provider protocol, model-checked (TLA+)

Status: **shipped (2026-09-06)** — M1–M4 done: specs/
ProviderProtocol.tla (+ kept mutant) with all seven named invariants
each validated by a throwaway mutant; Dockerfile `model` target gates
BOTH outcomes (base clean, mutant killed by CorrelationSafety);
compose `model` + ci.yml step wired. Doc gaps settled: (a) ids are
monotonic per session, never reused — late replies match no live
slot; (b) retry bounds are per caller (one attempt), unbounded per
session, ladder advancing only across successful rebuilds — as the
code does. Standing rule added to doc/provider-protocol.md. Bridge
tests in crates/stdio/src/tests.rs (5 fault classes), teeth proven by
reader-misroute mutation. Gate teeth proven by re-breaking the mutant
as a Die fault (killed via RestartFailClosed, not the grepped
CorrelationSafety → build failed) and reverting.

## Problem

`doc/provider-protocol.md` v1.5 is normative prose that three external
adapter authors implement from, and its core is a genuinely concurrent
state machine: reader thread, out-of-order id-routed replies,
`$/partial` streaming under inactivity deadlines, advisory
`$/cancelRequest`, child death → rebuild with bounded backoff and
single-rebuilder coordination. Every rule below is stated in English
and has never been machine-checked:

- "all `$/partial` for an id precede that id's reply. After the
  reply, no more for that id"
- "a reply that never come fails that one call … the late reply is
  discarded when it finally arrives"
- "at most one caller waits out a given rebuild attempt"

## Research findings

- strop's pattern (specs/EditorProtocol.tla + _Mutant.tla +
  cfg/*.cfg, plan 0024 there): a ~170-line spec, tiny bounds
  (DOCS={1,2}, REQS={1,2}, MAXREV=3), invariants named after the
  failure they forbid, and a kept mutant whose guard is removed —
  "TLC must FAIL this … if it ever passes, the invariant lost its
  teeth."
- TLC wiring: `eclipse-temurin:21-jre` base, sha256-pinned
  tla2tools 1.8.0, runs at image-build time — "the image existing IS
  the gate" (strop Dockerfile `model` target + compose `model`
  service + a ci.yml step).
- **Gap in strop's practice to not copy**: their gate runs only the
  base spec; the mutant is unchecked and can rot. Rootle gates both.
- The model-to-code bridge in strop is a model-based conformance
  harness; rootle's equivalent is the wire-level test: every fault
  class the model names becomes a stdio transport test
  (wiremock/echo-server) and a forge-conformance case shape.

## The spec

`specs/ProviderProtocol.tla` models the transport lifecycle, not
payload shapes: per-request state (pending → partial* →
result|error|timeout|cancelled), the reader thread, a discrete
deadline clock, child death/rebuild with backoff states, and the id
allocator. Invariants, named for the failure they forbid:

- **CorrelationSafety** — replies only for live ids
- **UniqueTerminal** — at most one result/error per id per session
- **PartialOrder** — no `$/partial` at or after terminal
- **TimeoutLiveness** — with fair delivery, every request reaches a
  terminal state client-side
- **RestartFailClosed** — child death errors every in-flight id
  exactly once; no zombie partials after rebuild
- **CancelAdvisory** — cancel breaks none of the above
- **NoIdReuse** — ids are monotonic per session (see gap (a))

Two prose gaps the model must settle (doc amended in the same PR):

- (a) "the late reply is discarded when it finally arrives" requires
  either monotonic id non-reuse or tombstones — the doc never says
  which; the implementation must be pinned to one.
- (b) rebuild retry: if attempt *k* fails, does the waiting caller
  ride attempt *k+1* indefinitely (30s cap each) or fail after a
  bound? Liveness question; settle and document.

## Milestones

### M1 — spec + mutant + gate

specs/ + cfg/; Dockerfile `model` target asserts BOTH: base spec
checks clean ("Model checking completed. No error") AND mutant fails
with the named invariant (inverted exit). compose `model` service;
ci.yml step beside `test`.

### M2 — prose fixes

Settle gaps (a)+(b) in doc/provider-protocol.md; if semantics change,
they ride v1.6 (0028's release) — reader tolerance makes tightening
client-side obligations safe for existing adapters.

### M3 — bridge tests

The fault classes as wire-level tests in the stdio crate (current
`src/provider/stdio/tests.rs`; moving with 0024 to
`crates/stdio/src/tests.rs` — check which path exists when writing):
double-final rejected, partial-after-final dropped, unknown-id
ignored, late-reply-after-timeout discarded (needs the (a) mechanism),
cancel-then-rebuild, EOF mid-stream. Conformance suite gains the
adapter-side mirrors.

### M4 — the standing rule

doc/provider-protocol.md gains: "any semantic change to transport,
cancellation, streaming, or restart updates specs/ and re-runs TLC in
the same PR."

## Honest scope (recorded pushback)

"Sound and complete" is not literally what TLC delivers and the
site/roadmap must not claim it. What ships: bounded-exhaustive
checking (≤3 in-flight, ≤4 partials, ≤2 rebuilds — seconds of CI),
mutant-validated invariants, and every modeled fault class pinned by
an executable test. The spec proves the protocol design; the bridge
tests hold the implementations to it.

## Verification

`docker compose run --build --rm model` green (both assertions);
mutant deliberately broken once more in review to watch the gate
fire.
