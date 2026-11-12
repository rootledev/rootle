//! UI ownership of request clocks. Only the event-loop thread mutates slots.

use super::{Generation, LoadPhase, LoadState, SearchGeneration, ViewGeneration};
use rootle_kernel::request::Reply;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlobLookup {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferenceLookup {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlameLookup {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HistoricalBlobLookup {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LastCommitLookup {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextDebounce {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HitContextLookup {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HitFileLookup {}

pub type BlobGeneration = Generation<BlobLookup>;
pub type ReferenceGeneration = Generation<ReferenceLookup>;
pub type BlameGeneration = Generation<BlameLookup>;
pub type HistoricalBlobGeneration = Generation<HistoricalBlobLookup>;
pub type LastCommitGeneration = Generation<LastCommitLookup>;
pub type DebounceGeneration = Generation<ContextDebounce>;
pub type HitContextGeneration = Generation<HitContextLookup>;
pub type HitFileGeneration = Generation<HitFileLookup>;

/// Read-only snapshot of the identities captured by data workers.
#[derive(Debug, Clone, Copy)]
pub struct RequestGenerations {
    pub blob: BlobGeneration,
    pub references: ReferenceGeneration,
    pub blame: BlameGeneration,
    pub historical_blob: HistoricalBlobGeneration,
    pub last_commit: LastCommitGeneration,
    pub debounce: DebounceGeneration,
    pub hit_context: HitContextGeneration,
    pub hit_file: HitFileGeneration,
}

#[derive(Debug)]
pub(crate) struct RequestSlot<Domain> {
    generation: Generation<Domain>,
    phase: LoadPhase,
}

impl<Domain> Default for RequestSlot<Domain> {
    fn default() -> Self {
        Self {
            generation: Generation::default(),
            phase: LoadPhase::Idle,
        }
    }
}

impl<Domain: Copy + PartialEq> RequestSlot<Domain> {
    pub fn issue(&mut self) -> Generation<Domain> {
        let generation = self.generation.tick();
        self.phase = LoadPhase::Loading;
        generation
    }

    pub fn current(&self) -> Generation<Domain> {
        self.generation
    }

    pub fn invalidate(&mut self) {
        // Keep the clock: the next issue must never reuse an old identity.
        self.phase = LoadPhase::Idle;
    }

    pub fn accept(&mut self, incoming: Generation<Domain>, reply: Reply) -> bool {
        if let Some(phase) = rootle_kernel::request::accept(
            self.generation.value(),
            self.phase.kernel(),
            incoming.value(),
            reply,
        ) {
            self.phase = LoadPhase::from_kernel(phase);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct AsyncRequests {
    pub search: LoadState<SearchGeneration>,
    pub view: LoadState<ViewGeneration>,
    pub blob: RequestSlot<BlobLookup>,
    pub references: RequestSlot<ReferenceLookup>,
    pub blame: RequestSlot<BlameLookup>,
    pub historical_blob: RequestSlot<HistoricalBlobLookup>,
    pub last_commit: RequestSlot<LastCommitLookup>,
    pub debounce: RequestSlot<ContextDebounce>,
    pub hit_context: RequestSlot<HitContextLookup>,
    pub hit_file: RequestSlot<HitFileLookup>,
}

impl AsyncRequests {
    pub fn generations(&self) -> RequestGenerations {
        RequestGenerations {
            blob: self.blob.current(),
            references: self.references.current(),
            blame: self.blame.current(),
            historical_blob: self.historical_blob.current(),
            last_commit: self.last_commit.current(),
            debounce: self.debounce.current(),
            hit_context: self.hit_context.current(),
            hit_file: self.hit_file.current(),
        }
    }

    pub fn invalidate_browser(&mut self) {
        self.blob.invalidate();
        self.references.invalidate();
        self.blame.invalidate();
        self.historical_blob.invalidate();
        self.last_commit.invalidate();
    }

    pub fn invalidate_view(&mut self) {
        self.view = LoadState::default();
        self.debounce.invalidate();
        self.hit_context.invalidate();
        self.hit_file.invalidate();
        self.blame.invalidate();
    }

    pub fn invalidate(&mut self) {
        self.search = LoadState::default();
        self.view = LoadState::default();
        self.invalidate_browser();
        self.debounce.invalidate();
        self.hit_context.invalidate();
        self.hit_file.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidation_and_reopening_never_revive_a_captured_identifier() {
        let mut slot = RequestSlot::<BlobLookup>::default();
        let first = slot.issue();
        slot.invalidate();
        assert!(!slot.accept(first, Reply::Complete));
        let reopened = slot.issue();
        assert!(reopened.value() > first.value());
        assert!(!slot.accept(first, Reply::Failed));
        assert!(slot.accept(reopened, Reply::Partial));
        assert!(slot.accept(reopened, Reply::Complete));
        assert!(!slot.accept(reopened, Reply::Complete));
        assert!(!slot.accept(reopened, Reply::Failed));
    }
}
