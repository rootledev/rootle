# 0027 — Provider protocol, model-checked (TLA+)

Status: **implemented and verified locally (2026-09-07)** — corrected
under 0029: advisory cancellation stays non-terminal, partials require
opt-in, admissions require a validated generation, and old readers are
isolated. TLC checks ten safety invariants, type correctness and two
fairness-qualified temporal properties. Four kept mutants must fail
by their named invariant and exit code, not parsing/runtime errors.
Generated traces exercise the production router; real-child scenarios
cover timeout, streaming, EOF and bounded recovery. Release tracking
lives in 0029; the earlier “shipped” wording was premature.

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

`specs/ProviderProtocol.tla` models admitted requests and recovery,
not payloads or OS pipe/process behavior. Cancellation records intent;
it does not complete a live request. Safety checks:

- `CorrelationSafety`, `UniqueTerminal`, `PartialOrder`, `PartialOptIn`
- `DeadlineBounded`, `RestartFailClosed`, `ValidatedAdmission`
- `CancelAdvisory`, `NoIdReuse`, `StaleReaderIsolation`, plus `TypeOK`

`EventuallyTerminal` requires finite streaming and weakly fair clock
and expiry scheduling. `RecoveryResolves` requires weakly fair
spawn/handshake scheduling. An indefinitely streaming real provider is
allowed not to terminate; a countdown bound is not a liveness proof.

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

Generated request/response/partial/expiry/restart traces exercise
`crates/stdio/src/routing.rs` through its real delivery channels.
Tests in `crates/stdio/src/tests.rs` and `restart/tests.rs` exercise
real child processes: duplicate/unknown/late replies, partial opt-in,
EOF fan-out, progressive recovery and failed-rebuild waiter bounds.
The external forge-conformance suite remains the adapter contract gate;
the new commit path is also exercised by the real-app headless suite.

### M4 — the standing rule

doc/provider-protocol.md gains: "any semantic change to transport,
cancellation, streaming, or restart updates specs/ and re-runs TLC in
the same PR."

## Honest scope (recorded pushback)

“Sound and complete” is not what TLC delivers. The checked configuration
uses two IDs, deadline two, one partial per request, one rebuild and a
backoff cap of two ticks. These are exhaustive finite bounds, not limits
imposed by the implementation. Four mutants validate fault detection;
the executable bridge is complementary evidence, not formal refinement.
OS backpressure/process-tree behavior and a full application-state model
are outside this model and recorded on the public roadmap.

## Verification

`docker compose run --build --rm model` passed with the corrected model.
The four faults are misrouting, terminal cancellation, unsolicited
partial delivery and stale-reader mutation. Rust and adapter gates
are recorded with the integration/release evidence in 0029.
