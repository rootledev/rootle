import Extracted.RootleKernel

/-!
The claims concern the actual extracted decision function, for all machine
integers and all phases/replies, not a second implementation. Payload identity,
nominal domain tags, dispatch wiring, scheduling and provider correctness remain
Rust adapter obligations. Cancellation is not an assumption of these proofs.
-/

open Aeneas Aeneas.Std Result
open rootle_kernel

namespace Rootle.Kernel

/-- Complete characterization of accepted replies and their resulting phase. -/
theorem accept_spec (current incoming : U64) (phase : request.Phase)
    (reply : request.Reply) :
    request.accept current phase incoming reply = ok (
      if current = incoming then
        match phase with
        | .Loading => some (match reply with
          | .Partial => .Loading
          | .Complete => .Ready
          | .Failed => .Failed)
        | .Idle | .Ready | .Failed => none
      else none) := by
  cases phase <;> cases reply <;> by_cases h : current = incoming <;>
    simp [request.accept, h]

/-- Superseded work never lands, regardless of its result or current phase. -/
theorem stale_rejected (current incoming : U64) (phase : request.Phase)
    (reply : request.Reply) (stale : current ≠ incoming) :
    request.accept current phase incoming reply = ok none := by
  simp [request.accept, stale]

/-- Idle and terminal requests refuse every subsequent reply. -/
theorem terminal_rejected (current incoming : U64) (phase : request.Phase)
    (reply : request.Reply)
    (closed : phase = .Idle ∨ phase = .Ready ∨ phase = .Failed) :
    request.accept current phase incoming reply = ok none := by
  rcases closed with rfl | rfl | rfl <;>
    by_cases h : current = incoming <;> simp [request.accept, h]

/-- Partial delivery preserves the right to accept later replies. -/
theorem partial_remains_loading (current : U64) :
    request.accept current .Loading current .Partial = ok (some .Loading) := by
  simp [request.accept]

/-- Success completes once; even a matching later partial/failure is refused. -/
theorem complete_once (current incoming : U64) (reply : request.Reply) :
    request.accept current .Loading current .Complete = ok (some .Ready) ∧
    request.accept current .Ready incoming reply = ok none := by
  by_cases h : current = incoming <;> simp [request.accept, h]

/-- Failure completes once and cannot subsequently turn into success. -/
theorem failure_once (current incoming : U64) (reply : request.Reply) :
    request.accept current .Loading current .Failed = ok (some .Failed) ∧
    request.accept current .Failed incoming reply = ok none := by
  by_cases h : current = incoming <;> simp [request.accept, h]

end Rootle.Kernel
