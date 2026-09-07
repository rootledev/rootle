---- MODULE ProviderProtocol ----
(**************************************************************************)
(* Bounded provider routing/recovery model (0027, corrected in 0029).       *)
(* Cancel is advisory: it records intent and leaves the request live.       *)
(* Only opted-in partials reset an inactivity deadline.                    *)
(* Every delivery records the pre-state independently of the routing       *)
(* guard; invariants check those observations, not ornamental flags.       *)
(* Fault selects a kept mutation; the release gate checks its counterexample.*)
(*                                                                        *)
(* Abstraction: an admitted application call starts after initialize.       *)
(* Handshake content, JSON payloads and OS process behavior are outside     *)
(* this model; Rust routing-conformance and real-child tests cover them.    *)
(* Tick models elapsed read time. MAXPARTIAL bounds finite streams.         *)
(* EventuallyTerminal relies on finite streaming plus weakly fair Tick     *)
(* and Expire, NOT merely on an armed countdown. A provider streaming      *)
(* forever is allowed by the real protocol and need not terminate.         *)
(**************************************************************************)
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS IDS, DEADLINE, MAXPARTIAL, MAXREBUILD, BACKOFFCAP, Fault
\* Observers retain the latest effect only. TLC checks each transition's
\* observation before it can be overwritten; accumulated history adds
\* combinations but changes neither the safety contract nor routing behavior.
VARIABLES requests, nextId, epoch, phase, output, backoff, rebuilds,
          deliveries, cancellations, readerEffects, admissions

vars == <<requests, nextId, epoch, phase, output, backoff, rebuilds,
          deliveries, cancellations, readerEffects, admissions>>
Statuses == {"free", "live", "result", "error", "timeout", "closed", "cancelled"}
Live == {id \in IDS : requests[id].status = "live"}
EmptyRequest == [status |-> "free", streaming |-> FALSE, remaining |-> 0,
                 partials |-> 0, completed |-> 0, issued |-> 0, epoch |-> 0]

Init ==
    /\ requests = [id \in IDS |-> EmptyRequest]
    /\ nextId = 0 /\ epoch = 0
    /\ phase = "ready" /\ output = "open"
    /\ backoff = 0 /\ rebuilds = 0
    /\ deliveries = {} /\ cancellations = {} /\ readerEffects = {} /\ admissions = {}

Finish(id, status) == [requests[id] EXCEPT !.status = status,
                      !.remaining = 0, !.completed = @ + 1]

Send(streaming) ==
    /\ phase = "ready" /\ output = "open"
    /\ nextId + 1 \in IDS
    /\ LET id == nextId + 1 IN
       /\ requests' = [requests EXCEPT ![id] =
           [status |-> "live", streaming |-> streaming, remaining |-> DEADLINE,
            partials |-> 0, completed |-> 0, issued |-> @.issued + 1, epoch |-> epoch]]
       /\ admissions' = {[id |-> id, validated |-> phase = "ready"]}
    /\ nextId' = nextId + 1
    /\ UNCHANGED <<epoch, phase, output, backoff, rebuilds, deliveries, cancellations, readerEffects>>

Observation(target, source, origin, kind) ==
    [owner |-> target, source |-> source, origin |-> origin,
     expectedEpoch |-> requests[target].epoch, live |-> target \in Live,
     streaming |-> requests[target].streaming, kind |-> kind]

Reply(source, origin, ok) ==
    /\ source \in IDS /\ origin \in 0..epoch
    /\ LET route == source \in Live /\ origin = epoch
           target == IF route THEN source
                     ELSE IF Fault = "misroute" /\ Live # {} THEN CHOOSE id \in Live : TRUE
                     ELSE 0
       IN IF target # 0
          THEN /\ requests' = [requests EXCEPT ![target] = Finish(target, IF ok THEN "result" ELSE "error")]
               /\ deliveries' = {Observation(target, source, origin, "response")}
          ELSE /\ requests' = requests /\ deliveries' = deliveries
    /\ UNCHANGED <<nextId, epoch, phase, output, backoff, rebuilds, cancellations, readerEffects, admissions>>

Partial(id, origin) ==
    /\ id \in IDS /\ origin \in 0..epoch
    /\ IF id \in Live /\ origin = epoch /\ requests[id].partials < MAXPARTIAL
          /\ (requests[id].streaming \/ Fault = "nonstream-partial")
       THEN /\ requests' = [requests EXCEPT ![id].remaining = DEADLINE, ![id].partials = @ + 1]
            /\ deliveries' = {Observation(id, id, origin, "partial")}
       ELSE /\ requests' = requests /\ deliveries' = deliveries
    /\ UNCHANGED <<nextId, epoch, phase, output, backoff, rebuilds, cancellations, readerEffects, admissions>>

Cancel(id) ==
    /\ id \in IDS
    /\ LET after == IF Fault = "cancel-terminal" /\ id \in Live THEN "cancelled" ELSE requests[id].status IN
       /\ requests' = [requests EXCEPT ![id].status = after]
       /\ cancellations' = {[before |-> requests[id].status, after |-> after]}
    /\ UNCHANGED <<nextId, epoch, phase, output, backoff, rebuilds, deliveries, readerEffects, admissions>>

Tick ==
    /\ (\E id \in Live : requests[id].remaining > 0) \/ backoff > 0
    /\ requests' = [id \in IDS |-> [requests[id] EXCEPT !.remaining = IF @ > 0 THEN @ - 1 ELSE 0]]
    /\ backoff' = IF backoff > 0 THEN backoff - 1 ELSE 0
    /\ UNCHANGED <<nextId, epoch, phase, output, rebuilds, deliveries, cancellations, readerEffects, admissions>>

Expire(id) ==
    /\ id \in Live /\ requests[id].remaining = 0
    /\ requests' = [requests EXCEPT ![id] = Finish(id, "timeout")]
    /\ UNCHANGED <<nextId, epoch, phase, output, backoff, rebuilds, deliveries, cancellations, readerEffects, admissions>>

Eof(origin) ==
    /\ origin \in 0..epoch
    /\ LET affects == origin = epoch \/ Fault = "stale-reader"
           nextLive == IF affects THEN {} ELSE Live
           nextPhase == IF affects /\ phase = "ready" THEN "dead" ELSE phase
           nextOutput == IF affects THEN "closed" ELSE output
       IN /\ requests' = [id \in IDS |-> IF affects /\ id \in Live THEN Finish(id, "closed") ELSE requests[id]]
          /\ phase' = nextPhase /\ output' = nextOutput
          /\ readerEffects' = {
              [stale |-> origin # epoch, before |-> Live, after |-> nextLive,
               phaseBefore |-> phase, phaseAfter |-> nextPhase,
               outputBefore |-> output, outputAfter |-> nextOutput]}
    /\ UNCHANGED <<nextId, epoch, backoff, rebuilds, deliveries, cancellations, admissions>>

StartRebuild ==
    /\ phase = "dead" /\ epoch < MAXREBUILD
    /\ phase' = "sleeping" /\ output' = "awaiting"
    /\ epoch' = epoch + 1
    /\ backoff' = IF rebuilds = 0 THEN 1 ELSE BACKOFFCAP
    /\ UNCHANGED <<requests, nextId, rebuilds, deliveries, cancellations, readerEffects, admissions>>

Spawn ==
    /\ phase = "sleeping" /\ backoff = 0
    /\ phase' = "handshake" /\ output' = "open"
    /\ UNCHANGED <<requests, nextId, epoch, backoff, rebuilds, deliveries, cancellations, readerEffects, admissions>>

HandshakeOk ==
    /\ phase = "handshake" /\ output = "open"
    /\ phase' = "ready" /\ rebuilds' = rebuilds + 1
    /\ UNCHANGED <<requests, nextId, epoch, output, backoff, deliveries, cancellations, readerEffects, admissions>>

HandshakeFail ==
    /\ phase = "handshake"
    /\ phase' = "dead" /\ output' = "closed"
    /\ UNCHANGED <<requests, nextId, epoch, backoff, rebuilds, deliveries, cancellations, readerEffects, admissions>>

Next ==
    \/ \E streaming \in BOOLEAN : Send(streaming)
    \/ \E id \in IDS, origin \in 0..epoch, ok \in BOOLEAN : Reply(id, origin, ok)
    \/ \E id \in IDS, origin \in 0..epoch : Partial(id, origin)
    \/ \E id \in IDS : Cancel(id)
    \/ \E id \in IDS : Expire(id)
    \/ Tick
    \/ \E origin \in 0..epoch : Eof(origin)
    \/ StartRebuild \/ Spawn \/ HandshakeOk \/ HandshakeFail

Spec == Init /\ [][Next]_vars
            /\ WF_vars(Tick) /\ (\A id \in IDS : WF_vars(Expire(id)))
            /\ WF_vars(Spawn) /\ WF_vars(HandshakeOk) /\ WF_vars(HandshakeFail)

TypeOK ==
    /\ requests \in [IDS -> [status: Statuses, streaming: BOOLEAN,
         remaining: 0..DEADLINE, partials: 0..MAXPARTIAL, completed: 0..2,
         issued: 0..1, epoch: 0..MAXREBUILD]]
    /\ epoch \in 0..MAXREBUILD /\ rebuilds \in 0..MAXREBUILD
    /\ phase \in {"ready", "dead", "sleeping", "handshake"}
    /\ output \in {"open", "closed", "awaiting"}
    /\ backoff \in 0..BACKOFFCAP /\ nextId \in {0} \cup IDS
CorrelationSafety == \A delivery \in deliveries :
    delivery.owner = delivery.source /\ delivery.origin = delivery.expectedEpoch /\ delivery.live
UniqueTerminal == \A id \in IDS : requests[id].completed <= 1
PartialOrder == \A delivery \in deliveries : delivery.kind = "partial" => delivery.live
PartialOptIn == \A delivery \in deliveries : delivery.kind = "partial" => delivery.streaming
DeadlineBounded == \A id \in Live : requests[id].remaining \in 0..DEADLINE
RestartFailClosed == output # "open" => Live = {}
ValidatedAdmission == \A admission \in admissions : admission.validated
CancelAdvisory == \A event \in cancellations : event.before = event.after
NoIdReuse == \A id \in IDS : requests[id].issued <= 1 /\ (id <= nextId => requests[id].status # "free")
StaleReaderIsolation == \A event \in readerEffects : event.stale =>
    event.before = event.after /\ event.phaseBefore = event.phaseAfter /\ event.outputBefore = event.outputAfter
EventuallyTerminal == \A id \in IDS : [](id \in Live => <>(id \notin Live))
RecoveryResolves == [](phase \in {"sleeping", "handshake"} => <>(phase \in {"ready", "dead"}))
=============================================================================
