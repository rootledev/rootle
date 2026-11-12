---- MODULE CommitHistory ----
(**************************************************************************)
(* Bounded abstraction of repo/log's opted-in append-only stream and its   *)
(* UI landing. Request numbers never repeat; messages may be delayed past  *)
(* supersession. Sent batches precede terminal metadata on each request.   *)
(* The source is finite; network failure is explicit. Weak fairness of     *)
(* delivery supplies progress, not an assumption that all providers live. *)
(* Payload parsing, HTTP and OS scheduling are covered by executable tests;*)
(* this model is not a Rust refinement proof.                              *)
(**************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets, TLC
CONSTANTS Budget, BatchSize, MaxRequests, MaxCommits, Fault
VARIABLES generation, phase, retained, clipped, truncated, requests,
          completions, acceptance, terminalObservation
variables == <<generation, phase, retained, clipped, truncated, requests,
               completions, acceptance, terminalObservation>>
RequestNumbers == 1..MaxRequests
EmptyRequest == [total |-> 0, sent |-> 0, terminal |-> FALSE, issued |-> FALSE]
Init ==
    /\ generation = 0 /\ phase = "idle" /\ retained = <<>>
    /\ clipped = FALSE /\ truncated = FALSE
    /\ requests = [request \in RequestNumbers |-> EmptyRequest]
    /\ completions = [request \in RequestNumbers |-> 0]
    /\ acceptance = {} /\ terminalObservation = {}

Start(total) ==
    /\ generation < MaxRequests /\ total \in 0..MaxCommits
    /\ generation' = generation + 1 /\ phase' = "loading"
    /\ retained' = <<>> /\ clipped' = FALSE /\ truncated' = FALSE
    /\ requests' = [requests EXCEPT ![generation + 1] =
        [total |-> total, sent |-> 0, terminal |-> FALSE, issued |-> TRUE]]
    /\ UNCHANGED <<completions, acceptance, terminalObservation>>

Accepts(request) == request = generation /\ phase = "loading"
CanLand(request) == Accepts(request) \/ Fault = "stale" \/
                    (Fault = "after-terminal" /\ request = generation)
Minimum(left, right) == IF left < right THEN left ELSE right

Batch(request) ==
    /\ requests[request].issued /\ ~requests[request].terminal
    /\ requests[request].sent < requests[request].total
    /\ LET sent == requests[request].sent
           count == Minimum(BatchSize, requests[request].total - sent)
           available == IF Fault = "unbounded" THEN count
                        ELSE Minimum(count, Budget - Len(retained))
           values == [index \in 1..available |-> sent + index]
       IN /\ requests' = [requests EXCEPT ![request].sent = sent + count]
          /\ IF CanLand(request)
             THEN /\ retained' = retained \o values
                  /\ clipped' = clipped \/ count > available
                  /\ acceptance' = {[source |-> request, current |-> generation,
                                      before |-> phase]}
             ELSE UNCHANGED <<retained, clipped, acceptance>>
    /\ UNCHANGED <<generation, phase, truncated, completions, terminalObservation>>

Finish(request, failed) ==
    /\ requests[request].issued /\ failed \in BOOLEAN
    /\ failed \/ requests[request].sent = requests[request].total
    /\ requests' = [requests EXCEPT ![request].terminal = TRUE]
    /\ IF CanLand(request)
       THEN /\ phase' = IF failed THEN "failed" ELSE "ready"
            /\ completions' = [completions EXCEPT ![request] = @ + 1]
            /\ truncated' = IF Fault = "forget-clipping" THEN FALSE ELSE clipped
            /\ retained' = IF failed /\ Fault = "erase-partials" THEN <<>> ELSE retained
            /\ acceptance' = {[source |-> request, current |-> generation, before |-> phase]}
            /\ terminalObservation' = {[failed |-> failed, before |-> retained,
                  after |-> retained', total |-> requests[request].total,
                  kept |-> Len(retained'), truncated |-> truncated']}
       ELSE UNCHANGED <<phase, completions, truncated, retained, acceptance, terminalObservation>>
    /\ UNCHANGED <<generation, clipped>>

Deliver(request) == Batch(request) \/ (\E failed \in BOOLEAN : Finish(request, failed))
Next == (\E total \in 0..MaxCommits : Start(total)) \/
        (\E request \in RequestNumbers : Deliver(request))
Spec == Init /\ [][Next]_variables /\
        (\A request \in RequestNumbers : WF_variables(Deliver(request)))

BoundedRetention == Len(retained) <= Budget
FreshLanding == \A observed \in acceptance : observed.source = observed.current
UniqueTerminal == \A request \in RequestNumbers : completions[request] <= 1
NoPostTerminal == \A observed \in acceptance : observed.before = "loading"
PrefixOrder == \A index \in 1..Len(retained) : retained[index] = index
TruthfulCompletion == \A observed \in terminalObservation :
    ~observed.failed /\ observed.kept < observed.total => observed.truncated
FailurePreservesPartials == \A observed \in terminalObservation :
    observed.failed => observed.before = observed.after
TypeOK == /\ generation \in 0..MaxRequests
          /\ phase \in {"idle", "loading", "ready", "failed"}
          /\ retained \in Seq(1..MaxCommits)
          /\ clipped \in BOOLEAN /\ truncated \in BOOLEAN
EventuallySettles == [](phase = "loading" => <>(phase # "loading"))
=============================================================================
