//! Commit detail vocabulary. Missing patches/counts are unknown, not proof
//! that a file is binary or that no lines changed.

use crate::id::{RepoPath, Sha};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct CommitFile {
    pub path: RepoPath,
    pub status: FileStatus,
    #[serde(default)]
    pub additions: Option<u32>,
    #[serde(default)]
    pub deletions: Option<u32>,
    #[serde(default)]
    pub patch: Option<String>,
    #[serde(default)]
    pub previous_path: Option<RepoPath>,
    /// True only when the backend has positively identified binary content.
    #[serde(default)]
    pub binary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Removed,
    Renamed,
    #[default]
    #[serde(other)]
    Modified,
}
impl FileStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Renamed => "renamed",
            Self::Modified => "modified",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct CommitDetail {
    pub sha: Sha,
    pub author: String,
    pub date: String,
    pub message: String,
    #[serde(default)]
    pub parents: Vec<Sha>,
    pub files: Vec<CommitFile>,
    /// The provider stopped at its file budget. This is not a complete list.
    #[serde(default)]
    pub truncated: bool,
    /// Provider-built permalink to this commit, not its repository homepage.
    #[serde(default)]
    pub web_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitStatistics {
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}
impl CommitDetail {
    /// Sum known counts without overflow; retain unknownness per side.
    pub fn line_stats(&self) -> CommitStatistics {
        CommitStatistics {
            additions: self.files.iter().try_fold(0u64, |total, file| {
                total.checked_add(u64::from(file.additions?))
            }),
            deletions: self.files.iter().try_fold(0u64, |total, file| {
                total.checked_add(u64::from(file.deletions?))
            }),
        }
    }
}
