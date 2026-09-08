//! Request identities follow work from dispatch to its guarded landing.
//! A repo-search generation cannot be mistaken for a file-search generation.

use rootle_provider::{Generation, GitRef, RepoId, RepoPath, Sha};

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

impl<Request: PartialEq> LoadState<Request> {
    pub fn start(&mut self, request: Request) {
        self.request = Some(request);
        self.phase = LoadPhase::Loading;
        self.error = None;
    }

    pub fn accepts(&self, request: &Request) -> bool {
        self.phase == LoadPhase::Loading && self.request.as_ref() == Some(request)
    }

    pub fn finish(&mut self, error: Option<&rootle_provider::ProviderError>) {
        self.phase = if error.is_some() {
            LoadPhase::Failed
        } else {
            LoadPhase::Ready
        };
        self.error = error.map(OperationError::from_provider);
    }
}
