//! Repository, organization and code search.

use super::{API, GitHubClient, urlencoding};
use crate::types::{OrgRepoItem, SearchReposResponse, SearchUsersResponse};
use rootle_provider::{ErrorKind, ProviderError, ProviderResult, SearchItem};

impl GitHubClient {
    /// Repo search + org search, merged: orgs first, then repos.
    /// Returns provider-level items (the trait boundary type).
    pub fn search(&self, query: &str) -> ProviderResult<Vec<SearchItem>> {
        let q = urlencoding(query);
        let mut out = Vec::new();

        let orgs: SearchUsersResponse =
            self.get(&format!("{API}/search/users?q={q}+type:org&per_page=5"))?;
        out.extend(orgs.items.into_iter().map(|u| SearchItem::Org(u.login)));

        let repos: SearchReposResponse =
            self.get(&format!("{API}/search/repositories?q={q}&per_page=20"))?;
        out.extend(
            repos
                .items
                .into_iter()
                .map(|r| SearchItem::Repo(r.full_name)),
        );

        Ok(out)
    }

    pub fn org_repos(&self, org: &str) -> ProviderResult<Vec<rootle_provider::RepoInfo>> {
        let repos: Vec<OrgRepoItem> =
            self.get(&format!("{API}/orgs/{org}/repos?per_page=100&sort=updated"))?;
        Ok(repos
            .into_iter()
            .map(|r| rootle_provider::RepoInfo {
                name: r.name,
                description: r.description,
                private: r.private,
                archived: r.archived,
                pushed_at: r.pushed_at,
            })
            .collect())
    }

    /// Code search (plans/0002 §4). Requires auth — anonymous clients
    /// get a clear error. `q` is the full query string including
    /// qualifiers (`repo:`, `org:`, `extension:`, `path:`); text-match
    /// fragments are requested for previews.
    pub fn search_code(&self, q: &str) -> ProviderResult<(Vec<crate::types::CodeItem>, bool)> {
        let (items, total) = self.search_code_page(q, 1)?;
        // GitHub caps code search at 1000 results — that is the
        // provider's own truncation signal (plans/0008 §4).
        Ok((items, total > 1000))
    }

    /// One page of code search (`per_page=100`, 1-indexed pages).
    /// Returns (items, total_count) — total drives `truncated` and the
    /// progressive page loop (v1.3, plans/0011).
    pub fn search_code_page(
        &self,
        q: &str,
        page: u32,
    ) -> ProviderResult<(Vec<crate::types::CodeItem>, u64)> {
        if self.is_anonymous() {
            return Err(ProviderError::new(
                ErrorKind::Auth,
                "code search needs a token — set ROOTLE_TOKEN or log in with `gh`",
            ));
        }
        let url = format!(
            "{API}/search/code?q={}&per_page=100&page={page}",
            urlencoding(q)
        );
        let resp: crate::types::SearchCodeResponse =
            self.get_accept(&url, "application/vnd.github.text-match+json")?;
        Ok((resp.items, resp.total_count))
    }
}
