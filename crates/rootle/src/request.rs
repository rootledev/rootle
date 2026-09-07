//! Request identities follow work from dispatch to its guarded landing.
//! A repo-search generation cannot be mistaken for a file-search generation.

use rootle_provider::{Generation, RepoId, Sha};

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
