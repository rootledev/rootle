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
