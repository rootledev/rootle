import Lean
import Kernel

/-!
# Axiom audit (request lifetime kernel)

Every theorem under `Rootle.Kernel` is audited: its transitive axioms must be a
subset of Lean's standard three. Anything else — `sorryAx`, a project axiom,
an Aeneas `axiom` standing in for an unmodelled external — fails the build.
The headline claims must also exist, so a deleted or renamed theorem cannot
pass silently. The `AXIOMS` lines this prints are the generated claim list.
-/
open Lean Elab Command

/-- Lean's standard axioms. -/
def allowedAxioms : List Name := [``propext, ``Classical.choice, ``Quot.sound]

/-- The headline claims (request lifetime kernel). -/
def claims : List Name := [
  ``Rootle.Kernel.next_generation_spec,
  ``Rootle.Kernel.accept_spec,
  ``Rootle.Kernel.stale_rejected,
  ``Rootle.Kernel.terminal_rejected,
  ``Rootle.Kernel.partial_remains_loading,
  ``Rootle.Kernel.complete_once,
  ``Rootle.Kernel.failure_once,
]

/-- Every theorem under `Rootle.Kernel`, sorted by name. -/
def kernelTheorems (env : Environment) : Array Name :=
  let audited : Name := `Rootle.Kernel
  (env.constants.fold (init := (#[] : Array Name)) fun acc decl info =>
    if audited.isPrefixOf decl && decl.isInternal = false && info.isThm then acc.push decl
    else acc).qsort (fun a b => a.toString < b.toString)

-- `name` is a reserved token once the Aeneas library is imported (its attribute
-- syntax), so declarations are called `decl` here.
def auditAxioms : CommandElabM Unit := do
  let env ← getEnv
  let sorted := kernelTheorems env
  if sorted.isEmpty then
    throwError "axiom audit: no theorems under Rootle.Kernel"
  for decl in sorted do
    let axioms := (← collectAxioms decl).qsort (fun a b => a.toString < b.toString)
    logInfo m!"AXIOMS {decl}: {axioms.toList}"
    for ax in axioms do
      unless allowedAxioms.contains ax do
        throwError "axiom audit: {decl} depends on {ax}, which is not a standard Lean axiom"
  for claim in claims do
    unless sorted.contains claim do
      throwError "axiom audit: required claim {claim} is missing or is not a theorem"
  logInfo m!"AXIOM_AUDIT ok: {sorted.size} theorems, {claims.length} required claims, axioms ⊆ {allowedAxioms}"

run_cmd auditAxioms
