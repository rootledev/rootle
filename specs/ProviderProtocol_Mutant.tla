---- MODULE ProviderProtocol_Mutant ----
(***************************************************************************)
(* KEPT MUTANT (0027): Deliver without the live-id/correlation guard —   *)
(* the reader applies a reply to whatever the id names: a completed,     *)
(* cancelled, timed-out, closed, or never-allocated slot. TLC must FAIL  *)
(* this with CorrelationSafety. If it ever passes, the invariant lost    *)
(* its teeth.                                                            *)
(***************************************************************************)
(***************************************************************************)
(* The rootle provider transport lifecycle (plans/0027), model-checked.   *)
(* doc/provider-protocol.md v1.5 is normative prose; this spec is its     *)
(* concurrency core, machine-checked with TLC:                             *)
(*                                                                       *)
(*   1. requests ride a monotonic id allocator (1, 2, 3, ... — never      *)
(*      reused, not across timeouts, not across rebuilds)                 *)
(*   2. a reader thread routes replies and $/partial notifications by    *)
(*      id; a delivery for an id with no live slot (late after timeout,  *)
(*      duplicate, unknown, after-cancel) is DROPPED, never applied      *)
(*   3. per-request lifecycle: pending -> partial* -> one terminal of    *)
(*      result | error (the reply), timeout (the read deadline),         *)
(*      cancelled (advisory $/cancelRequest, client abandons),          *)
(*      closed (child death)                                             *)
(*   4. the deadline is discrete and per-inactivity: Tick counts it      *)
(*      down for every live request at once; a $/partial or the reply    *)
(*      re-arms it — never extends it past one deadline period           *)
(*   5. child death closes every in-flight request; the next request     *)
(*      rebuilds after a backoff rung (the ladder advances only when a   *)
(*      rebuild succeeded and the child later died again), then the      *)
(*      replacement must pass initialize before it serves anything       *)
(*                                                                       *)
(* THE invariants, each named after the failure it forbids:              *)
(*   CorrelationSafety — replies/partials are ROUTED only to live ids;   *)
(*                       unknown, late, and duplicate deliveries drop    *)
(*   UniqueTerminal    — at most one terminal transition per id          *)
(*   PartialOrder      — no $/partial routed at/after an id's terminal   *)
(*   TimeoutLiveness   — every live request holds an armed countdown     *)
(*                       within one deadline period (safety-encoded:     *)
(*                       plus always-enabled Tick/Expire, every request  *)
(*                       reaches a terminal client-side)                 *)
(*   RestartFailClosed — child down means nothing in flight; death       *)
(*                       closes every in-flight id exactly once          *)
(*   CancelAdvisory    — a cancel ends its request only as `cancelled`; *)
(*                       result/error come from the reply alone          *)
(*   NoIdReuse         — ids at or below the allocator's high-water      *)
(*                       mark are never free again                        *)
(*                                                                       *)
(* Not modeled: payload shapes (wire.rs's job), real time (one Tick =    *)
(* one deadline quantum), multiple providers/sessions, the initialize    *)
(* handshake's content (only that a rebuild must validate — Restart-     *)
(* FailClosed), and liveness fairness (bounded-exhaustive safety check   *)
(* only — see plans/0027 "Honest scope").                                *)
(***************************************************************************)
EXTENDS Integers, FiniteSets

CONSTANTS IDS,         \* request ids in play, e.g. {1, 2, 3}
          DEADLINE,    \* deadline period in Ticks, e.g. 3
          MAXPARTIAL,  \* $/partial batches per request before the reply
          MAXREBUILD,  \* cap on successful rebuilds (bounds the model)
          BACKOFFCAP   \* longest backoff rung in Ticks

VARIABLES req,         \* id -> [state, remaining, partials, done, term]
          nextId,      \* allocator high-water mark (ids 1..nextId spent)
          child,       \* up | down (down = reader saw EOF, needs rebuild)
          backoff,     \* Ticks until a rebuild may run
          rebuilds,    \* successful rebuilds so far (drives the rung)
          misrouted,   \* TRUE iff a delivery was ROUTED onto a non-live
                       \* slot — unreachable in the base spec; THIS
                       \* mutant's unguarded Deliver sets it
          zombiePartials \* $/partial routed at/after terminal — zero by
                       \* construction; a spec edit that routes partials
                       \* onto dead slots drives it off zero

up == "up"
down == "down"

free == "free"
pending == "pending"
partial == "partial"
result == "result"
error == "error"
timeout == "timeout"
cancelled == "cancelled"
closed == "closed"

none == "none"
reply == "reply"
expire == "expire"
cancel == "cancel"
death == "death"

STATES == {free, pending, partial, result, error, timeout, cancelled, closed}
TERMS == {none, reply, expire, cancel, death}

Live(id) == req[id].state \in {pending, partial}

\* The backoff rung for the coming rebuild attempt: the ladder advances
\* only when a rebuild succeeded and the child later died again (the
\* code's `restarts` counter); a failing streak retries its rung.
\* Scaled: real 1s -> 2s -> 5s -> 30s cap becomes 1 -> BACKOFFCAP.
BackoffFor(attempt) == IF attempt <= 1 THEN 1 ELSE BACKOFFCAP

\* Terminal transition for id: one state, one source tag, done counts
\* terminal landings (UniqueTerminal caps it at 1).
Terminate(id, st, how) ==
    [state |-> st, remaining |-> 0, partials |-> req[id].partials,
     done |-> req[id].done + 1, term |-> how]

TypeOK ==
    /\ req \in [IDS -> [state: STATES,
                        \* loose by two so the one-deadline-period bound
                        \* is enforced by TimeoutLiveness, not the types
                        remaining: 0..DEADLINE + 2,
                        partials: 0..MAXPARTIAL,
                        done: 0..2,
                        term: TERMS]]
    /\ nextId \in {0} \cup IDS
    /\ child \in {up, down}
    /\ backoff \in 0..BACKOFFCAP
    /\ rebuilds \in 0..MAXREBUILD
    /\ misrouted \in BOOLEAN
    /\ zombiePartials \in 0..MAXPARTIAL

Init ==
    /\ req = [id \in IDS |-> [state |-> free, remaining |-> 0,
                              partials |-> 0, done |-> 0, term |-> none]]
    /\ nextId = 0
    /\ child = up
    /\ backoff = 0
    /\ rebuilds = 0
    /\ misrouted = FALSE
    /\ zombiePartials = 0

\* A fresh id leaves the allocator: armed with one full deadline period.
Send ==
    /\ child = up                    \* gated: nothing rides a dead child
    /\ nextId + 1 \in IDS
    /\ nextId' = nextId + 1
    /\ req' = [req EXCEPT ![nextId + 1] =
        [state |-> pending, remaining |-> DEADLINE,
         partials |-> 0, done |-> 0, term |-> none]]
    /\ UNCHANGED <<child, backoff, rebuilds, misrouted, zombiePartials>>

\* KEPT MUTANT FAULT: the live-id/correlation guard is gone. The base
\* spec drops a delivery that matches no live slot; this one applies it
\* anywhere the id points — misrouted flips TRUE on the first such
\* delivery and done double-increments on a terminal slot.
Deliver(id, ok) ==
    /\ id \in IDS
    /\ req' = [req EXCEPT ![id] =
                   Terminate(id, IF ok THEN result ELSE error, reply)]
    /\ misrouted' = IF Live(id) THEN misrouted ELSE TRUE
    /\ UNCHANGED <<nextId, child, backoff, rebuilds, zombiePartials>>

\* A $/partial batch: routed only to a live slot, and it re-arms (never
\* extends) the inactivity deadline — a stream that keeps talking never
\* times out, a silent one still does.
Partial(id) ==
    /\ id \in IDS
    /\ IF Live(id) /\ req[id].partials < MAXPARTIAL
       THEN /\ req' = [req EXCEPT ![id] =
               [req[id] EXCEPT !.state = partial, !.remaining = DEADLINE,
                                !.partials = @ + 1]]
            /\ zombiePartials' = zombiePartials
       ELSE /\ req' = req
            /\ zombiePartials' = zombiePartials
    /\ UNCHANGED <<nextId, child, backoff, rebuilds, misrouted>>

\* Advisory $/cancelRequest: the client abandons a live request; the
\* reply may still arrive — and is then an ordinary non-live delivery,
\* dropped above. Cancels for unknown or completed ids are no-ops.
Cancel(id) ==
    /\ id \in IDS
    /\ req' = IF Live(id)
              THEN [req EXCEPT ![id] = Terminate(id, cancelled, cancel)]
              ELSE req
    /\ UNCHANGED <<nextId, child, backoff, rebuilds, misrouted, zombiePartials>>

\* The read deadline fired: the slot is dropped (never reused), the
\* transport stays usable, and the eventual late reply is discarded.
Expire(id) ==
    /\ Live(id)
    /\ req[id].remaining = 1
    /\ req' = [req EXCEPT ![id] = Terminate(id, timeout, expire)]
    /\ UNCHANGED <<nextId, child, backoff, rebuilds, misrouted, zombiePartials>>

\* One deadline quantum passes: every live request's countdown ticks
\* toward its expiry; the rebuild gate ticks open.
Tick ==
    /\ \/ \E id \in IDS : Live(id) /\ req[id].remaining > 1
       \/ backoff > 0
    /\ req' = [id \in IDS |-> [req[id] EXCEPT !.remaining = IF @ > 1 THEN @ - 1 ELSE @]]
    /\ backoff' = IF backoff > 0 THEN backoff - 1 ELSE 0
    /\ UNCHANGED <<nextId, child, rebuilds, misrouted, zombiePartials>>

\* The child dies (EOF on its stdout): every in-flight request fails
\* closed, exactly once, and nothing new rides the dead transport.
Die ==
    /\ child = up
    /\ child' = down
    /\ backoff' = BackoffFor(rebuilds + 1)
    /\ req' = [id \in IDS |->
        IF Live(id)
        THEN [req[id] EXCEPT !.state = closed, !.remaining = 0,
                             !.done = @ + 1, !.term = death]
        ELSE req[id]]
    /\ UNCHANGED <<nextId, rebuilds, misrouted, zombiePartials>>

\* A rebuild attempt that validated (passed initialize): the transport
\* serves again — fresh ids only; the pre-death requests stay closed.
RebuildOk ==
    /\ child = down
    /\ backoff = 0
    /\ rebuilds < MAXREBUILD
    /\ child' = up
    /\ rebuilds' = rebuilds + 1
    /\ UNCHANGED <<req, nextId, backoff, misrouted, zombiePartials>>

\* A rebuild attempt that failed (spawn error, failed handshake): same
\* rung again for the next fresh request — retries are unbounded per
\* session; the bound is per caller (the code fails waiters after one
\* attempt, which Tick/Expire never see: Die already closed everything).
RebuildFail ==
    /\ child = down
    /\ backoff = 0
    /\ backoff' = BackoffFor(rebuilds + 1)
    /\ UNCHANGED <<req, nextId, child, rebuilds, misrouted, zombiePartials>>

Next ==
    \/ Send
    \/ \E id \in IDS, ok \in {TRUE, FALSE} : Deliver(id, ok)
    \/ \E id \in IDS : Partial(id)
    \/ \E id \in IDS : Cancel(id)
    \/ \E id \in IDS : Expire(id)
    \/ Tick
    \/ Die
    \/ RebuildOk
    \/ RebuildFail

Spec == Init /\ [][Next]_<<req, nextId, child, backoff, rebuilds,
                         misrouted, zombiePartials>>

(* -- the invariants ------------------------------------------------------- *)

CorrelationSafety == ~misrouted

UniqueTerminal ==
    \A id \in IDS : req[id].done <= 1

PartialOrder ==
    zombiePartials = 0

TimeoutLiveness ==
    \A id \in IDS : Live(id) => req[id].remaining \in 1..DEADLINE

RestartFailClosed ==
    (child = down) => \A id \in IDS : ~Live(id)

CancelAdvisory ==
    \A id \in IDS :
        /\ req[id].state \in {result, error} => req[id].term = reply
        /\ req[id].state = cancelled => req[id].term = cancel
        /\ req[id].state = timeout => req[id].term = expire
        /\ req[id].state = closed => req[id].term = death
        /\ req[id].state \in {pending, partial} =>
            req[id].done = 0 /\ req[id].term = none

NoIdReuse ==
    \A id \in IDS : id <= nextId => req[id].state # free

THEOREM Spec => []TypeOK
          /\ []CorrelationSafety /\ []UniqueTerminal /\ []PartialOrder
          /\ []TimeoutLiveness /\ []RestartFailClosed
          /\ []CancelAdvisory /\ []NoIdReuse
=============================================================================
