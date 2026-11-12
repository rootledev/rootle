import Extracted.RootleKernel

open Aeneas Aeneas.Std Result
open rootle_kernel

namespace Rootle.Kernel

/-- `u64::MAX` as a value. -/
theorem u64_MAX_val : (core.num.U64.MAX).val = U64.max := by
  simp [core.num.U64.MAX, U64.rMax, U64.max_eq]

/-- `next_generation` advances by one, or refuses issuance at `u64::MAX`. -/
theorem next_generation_spec (current : U64) :
    generation.next_generation current ⦃ r =>
      (r.isSome ↔ current.val < U64.max) ∧
      (∀ next, r = some next → next.val = current.val + 1) ⦄ := by
  unfold generation.next_generation
  split_ifs with h
  · have : current.val = U64.max := by rw [h, u64_MAX_val]
    simp [this]
  · have hne : current.val ≠ U64.max := by
      intro heq; apply h
      apply UScalar.eq_of_val_eq
      rw [heq, u64_MAX_val]
    have hlt : current.val < U64.max := by scalar_tac
    step as ⟨ next, hnext ⟩
    simp [hlt, hnext]


end Rootle.Kernel
