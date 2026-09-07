//! Branch/tag discovery.

use super::{API, GitHubClient};
use crate::types::RepoMeta;
use rootle_provider::ProviderResult;

impl GitHubClient {
    /// Branches (first 100) + tags (first 100); the repo's default
    /// branch is marked. Refs past the cap are out of scope for a
    /// switcher.
    pub fn refs(&self, owner: &str, repo: &str) -> ProviderResult<rootle_provider::RepoRefs> {
        #[derive(serde::Deserialize)]
        struct BranchItem {
            name: String,
            commit: CommitRef,
        }
        #[derive(serde::Deserialize)]
        struct CommitRef {
            sha: String,
        }
        #[derive(serde::Deserialize)]
        struct TagRef {
            #[serde(rename = "ref")]
            name: String, // "refs/tags/v1.0"
            object: CommitRef,
        }
        let meta: RepoMeta = self.get(&format!("{API}/repos/{owner}/{repo}"))?;
        let branches: Vec<BranchItem> =
            self.get(&format!("{API}/repos/{owner}/{repo}/branches?per_page=100"))?;
        let tags: Vec<TagRef> = self.get(&format!(
            "{API}/repos/{owner}/{repo}/git/refs/tags?per_page=100"
        ))?;
        Ok(rootle_provider::RepoRefs {
            branches: branches
                .into_iter()
                .map(|b| rootle_provider::RefInfo {
                    is_default: b.name == meta.default_branch,
                    name: b.name,
                    sha: b.commit.sha,
                })
                .collect(),
            tags: tags
                .into_iter()
                .map(|t| rootle_provider::RefInfo {
                    name: t.name.trim_start_matches("refs/tags/").to_string(),
                    sha: t.object.sha,
                    is_default: false,
                })
                .collect(),
        })
    }
}
