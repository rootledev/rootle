//! Request identities follow work from dispatch to its guarded landing.
//! A repo-search generation cannot be mistaken for a file-search generation.

use rootle_kernel::request::{Phase, Reply};
use rootle_provider::{GitRef, RepoId, RepoPath, Sha};

mod lifecycle;
pub(crate) use lifecycle::AsyncRequests;
pub use lifecycle::{
    BlameGeneration, BlobGeneration, DebounceGeneration, HistoricalBlobGeneration,
    HitContextGeneration, HitFileGeneration, LastCommitGeneration, ReferenceGeneration,
    RequestGenerations,
};

/// The domain tag prevents cross-pipeline comparisons; only the owning UI
/// clock advances. Serialization belongs here, not in the extracted kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Generation<Domain> {
    value: u64,
    domain: std::marker::PhantomData<Domain>,
}

impl<Domain> Default for Generation<Domain> {
    fn default() -> Self {
        Self {
            value: 0,
            domain: std::marker::PhantomData,
        }
    }
}

impl<Domain: Copy + PartialEq> Generation<Domain> {
    /// Exhaustion is fatal rather than ever handing out a reused identifier.
    pub fn tick(&mut self) -> Self {
        self.value = rootle_kernel::generation::next_generation(self.value)
            .expect("request generation exhausted");
        *self
    }

    pub fn is_current(&self, other: Self) -> bool {
        *self == other
    }
}

impl<Domain> Generation<Domain> {
    pub fn value(&self) -> u64 {
        self.value
    }
}

impl<Domain> std::fmt::Display for Generation<Domain> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(formatter)
    }
}

impl<Domain> serde::Serialize for Generation<Domain> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepositorySearch {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentSearch {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommitLookup {}

pub type SearchGeneration = Generation<RepositorySearch>;
pub type ViewGeneration = Generation<ContentSearch>;
pub type CommitGeneration = Generation<CommitLookup>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRequest {
    pub repository: RepoId,
    pub revision: Sha,
    pub generation: CommitGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HistoryLookup {}
pub type HistoryGeneration = Generation<HistoryLookup>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum HistoryScope {
    Repository,
    File(RepoPath),
}

impl HistoryScope {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Repository => None,
            Self::File(path) => Some(path.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HistoryRequest {
    pub repository: RepoId,
    pub revision: Option<GitRef>,
    pub scope: HistoryScope,
    pub generation: HistoryGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TreeLookup {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnerLookup {}
pub type TreeGeneration = Generation<TreeLookup>;
pub type OwnerGeneration = Generation<OwnerLookup>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TreeRequest {
    pub repository: RepoId,
    pub revision: Option<GitRef>,
    pub generation: TreeGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OwnerListRequest {
    pub owner: rootle_provider::OwnerId,
    pub generation: OwnerGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ContentSearchRequest {
    pub generation: ViewGeneration,
    pub kind: crate::components::global_search::SearchKind,
    pub query: String,
    /// The submitted provider query qualifier, independent of subsequently edited fields.
    pub scope: String,
    pub extension: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed,
}

impl LoadPhase {
    fn kernel(self) -> Phase {
        match self {
            Self::Idle => Phase::Idle,
            Self::Loading => Phase::Loading,
            Self::Ready => Phase::Ready,
            Self::Failed => Phase::Failed,
        }
    }

    fn from_kernel(phase: Phase) -> Self {
        match phase {
            Phase::Idle => Self::Idle,
            Phase::Loading => Self::Loading,
            Phase::Ready => Self::Ready,
            Phase::Failed => Self::Failed,
        }
    }
}

/// Payload identity and epoch both have to match before a reply can land.
pub(crate) trait RequestIdentity: PartialEq {
    fn generation(&self) -> u64;
}

impl<Domain: PartialEq> RequestIdentity for Generation<Domain> {
    fn generation(&self) -> u64 {
        self.value
    }
}

macro_rules! request_identity {
    ($($request:ty),+ $(,)?) => {
        $(impl RequestIdentity for $request {
            fn generation(&self) -> u64 {
                self.generation.value()
            }
        })+
    };
}

request_identity!(
    CommitRequest,
    HistoryRequest,
    TreeRequest,
    OwnerListRequest,
    ContentSearchRequest,
);

/// Safe UI/observer projection. Classification survives; rendering never re-parses prose.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OperationError {
    pub kind: rootle_provider::ErrorKind,
    pub message: String,
    pub retry_after_s: Option<u64>,
}

impl OperationError {
    pub fn from_provider(error: &rootle_provider::ProviderError) -> Self {
        let end = error.message.char_indices().nth(2048).map(|(i, _)| i);
        let mut message =
            crate::sanitize::sanitize_inline(&error.message[..end.unwrap_or(error.message.len())]);
        if end.is_some() {
            message.push('…');
        }
        Self {
            kind: error.kind,
            message,
            retry_after_s: error.retry_after.map(|duration| duration.as_secs()),
        }
    }

    pub fn kind_name(&self) -> &'static str {
        self.kind.as_str()
    }
}

/// One domain's current request, not a worker counter or global status string.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct LoadState<Request> {
    pub request: Option<Request>,
    pub phase: LoadPhase,
    pub error: Option<OperationError>,
}

impl<Request> Default for LoadState<Request> {
    fn default() -> Self {
        Self {
            request: None,
            phase: LoadPhase::Idle,
            error: None,
        }
    }
}

impl<Request: RequestIdentity> LoadState<Request> {
    pub fn start(&mut self, request: Request) {
        self.request = Some(request);
        self.phase = LoadPhase::Loading;
        self.error = None;
    }

    pub fn accepts(&self, request: &Request) -> bool {
        self.request.as_ref().is_some_and(|current| {
            current == request
                && rootle_kernel::request::accept(
                    current.generation(),
                    self.phase.kernel(),
                    request.generation(),
                    Reply::Partial,
                )
                .is_some()
        })
    }

    pub fn receive(&mut self, request: &Request, reply: Reply) -> bool {
        let Some(current) = self.request.as_ref().filter(|current| *current == request) else {
            return false;
        };
        if let Some(phase) = rootle_kernel::request::accept(
            current.generation(),
            self.phase.kernel(),
            request.generation(),
            reply,
        ) {
            self.phase = LoadPhase::from_kernel(phase);
            true
        } else {
            false
        }
    }

    pub fn finish(&mut self, error: Option<&rootle_provider::ProviderError>) {
        let Some(request) = &self.request else {
            return;
        };
        let reply = if error.is_some() {
            Reply::Failed
        } else {
            Reply::Complete
        };
        if let Some(phase) = rootle_kernel::request::accept(
            request.generation(),
            self.phase.kernel(),
            request.generation(),
            reply,
        ) {
            self.phase = LoadPhase::from_kernel(phase);
            self.error = error.map(OperationError::from_provider);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superseded_partial_and_duplicate_terminal_do_not_land() {
        let mut clock = ViewGeneration::default();
        let old = clock.tick();
        let mut load = LoadState::default();
        load.start(old);
        assert!(load.receive(&old, Reply::Partial));
        let current = clock.tick();
        load.start(current);
        assert!(!load.receive(&old, Reply::Complete));
        assert!(load.receive(&current, Reply::Partial));
        assert!(load.receive(&current, Reply::Failed));
        assert!(!load.receive(&current, Reply::Complete));
        assert!(!load.receive(&current, Reply::Partial));
        assert_eq!(load.phase, LoadPhase::Failed);
    }

    #[test]
    fn payload_identity_is_checked_in_addition_to_generation() {
        let request = TreeRequest {
            repository: "first/repo".into(),
            revision: None,
            generation: TreeGeneration::default().tick(),
        };
        let mut other = request.clone();
        other.repository = "second/repo".into();
        let mut load = LoadState::default();
        load.start(request.clone());
        assert!(!load.receive(&other, Reply::Complete));
        assert!(load.receive(&request, Reply::Complete));
        assert!(!load.accepts(&request));
    }

    #[test]
    fn generation_exhaustion_refuses_without_changing_the_clock() {
        let mut clock = Generation::<RepositorySearch> {
            value: u64::MAX,
            domain: std::marker::PhantomData,
        };
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| clock.tick())).is_err());
        assert_eq!(clock.value(), u64::MAX);
    }
}
