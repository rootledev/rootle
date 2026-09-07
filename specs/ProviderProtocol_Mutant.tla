---- MODULE ProviderProtocol_Mutant ----
(* Kept mutations are selected by Fault in each config. All transitions
   and observations are shared with the production model, not duplicated.
   check.sh requires an actual invariant violation for each named fault. *)
EXTENDS ProviderProtocol
=============================================================================
