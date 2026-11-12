//! Commit history and commit-detail retrieval.

use super::transport::Page;
use super::{API, GitHubClient};
use rootle_provider::{LogEntry, ProviderError, ProviderResult, RepoId};
use std::collections::HashSet;

#[cfg(test)]
mod tests;

#[derive(serde::Deserialize)]
struct HistoryCommit {
    sha: String,
    commit: HistoryMetadata,
}

#[derive(serde::Deserialize)]
struct HistoryMetadata {
    message: String,
    author: HistoryAuthor,
}

#[derive(serde::Deserialize)]
struct HistoryAuthor {
    name: String,
    date: String,
}

impl GitHubClient {
    /// Page history at one pinned revision; stream each bounded page to the UI.
    pub fn log(
        &self,
        repository: &RepoId,
        path: Option<&str>,
        revision: Option<&str>,
        limit: Option<usize>,
        on_entries: &mut dyn FnMut(Vec<LogEntry>),
    ) -> ProviderResult<bool> {
        let (owner, repository_name) = crate::split_repo(repository)?;
        let mut url = reqwest::Url::parse(API).expect("GitHub API URL");
        url.path_segments_mut()
            .expect("HTTP URL supports path segments")
            .extend(["repos", owner, repository_name, "commits"]);
        stream_history(
            url,
            path,
            revision,
            limit.unwrap_or(50).min(rootle_provider::RENDER_BUDGET),
            on_entries,
            |url| self.get_page(url),
        )
    }

    /// v1.6 (plans/0028 M1): one commit's detail — message, parents,
    /// changed files with their unified hunks. The wire model is
    /// `types::CommitResponse`; the seam mapping lives in lib.rs.
    pub fn commit(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> ProviderResult<crate::types::CommitResponse> {
        const FILES_PER_PAGE: usize = 100;
        const FILE_BUDGET: usize = rootle_provider::RENDER_BUDGET;
        let mut url = reqwest::Url::parse(API).expect("GitHub API URL");
        url.path_segments_mut()
            .expect("HTTP URL supports path segments")
            .extend(["repos", owner, repo, "commits", sha]);
        url.query_pairs_mut()
            .append_pair("per_page", &FILES_PER_PAGE.to_string());
        let first = self.get_page::<crate::types::CommitResponse>(url.as_str())?;
        let mut response = first.body;
        let mut has_next = first.has_next;
        let mut page = 1u32;
        // Pagination is file-only. Pin subsequent pages to the resolved
        // content ID, even if a caller supplied a resolvable revision.
        url.path_segments_mut()
            .expect("HTTP URL supports path segments")
            .pop()
            .push(&response.sha);
        while has_next && response.files.len() < FILE_BUDGET {
            page += 1;
            url.query_pairs_mut()
                .clear()
                .append_pair("per_page", &FILES_PER_PAGE.to_string())
                .append_pair("page", &page.to_string());
            let next = self.get_page::<crate::types::CommitResponse>(url.as_str())?;
            if next.body.files.is_empty() && next.has_next {
                return Err(ProviderError::other(
                    "commit file pagination made no progress",
                ));
            }
            if next.body.sha != response.sha {
                return Err(rootle_provider::ProviderError::other(
                    "commit changed across file pages",
                ));
            }
            response.files.extend(next.body.files);
            has_next = next.has_next;
        }
        response.truncated = has_next || response.files.len() > FILE_BUDGET;
        response.files.truncate(FILE_BUDGET);
        Ok(response)
    }
}

fn stream_history(
    mut url: reqwest::Url,
    path: Option<&str>,
    revision: Option<&str>,
    limit: usize,
    on_entries: &mut dyn FnMut(Vec<LogEntry>),
    mut fetch: impl FnMut(&str) -> ProviderResult<Page<Vec<HistoryCommit>>>,
) -> ProviderResult<bool> {
    // Keep page size constant: changing it between numbered pages skips entries.
    let page_size = limit.clamp(1, 100);
    let mut pinned_revision = revision.map(str::to_owned);
    let mut retained = 0usize;
    let mut seen = HashSet::new();
    for page_number in 1..=limit.div_ceil(page_size).max(1) {
        let mut query = url.query_pairs_mut();
        query
            .clear()
            .append_pair("per_page", &page_size.to_string())
            .append_pair("page", &page_number.to_string());
        if let Some(path) = path {
            query.append_pair("path", path);
        }
        if let Some(revision) = &pinned_revision {
            query.append_pair("sha", revision);
        }
        drop(query);
        let page = fetch(url.as_str())?;
        if page.body.is_empty() && page.has_next {
            return Err(ProviderError::other("history pagination made no progress"));
        }
        if page_number == 1
            && let Some(first) = page.body.first()
        {
            pinned_revision = Some(first.sha.clone());
        }
        let clipped = page.body.len() > limit - retained;
        let mut entries = Vec::with_capacity(page.body.len().min(limit - retained));
        for commit in page.body.into_iter().take(limit - retained) {
            if !seen.insert(commit.sha.clone()) {
                return Err(ProviderError::other("history pagination repeated a commit"));
            }
            entries.push(LogEntry {
                sha: commit.sha,
                subject: commit
                    .commit
                    .message
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                author: commit.commit.author.name,
                date: commit.commit.author.date,
            });
        }
        retained += entries.len();
        if !entries.is_empty() {
            on_entries(entries);
        }
        if !page.has_next || retained == limit {
            return Ok(page.has_next || clipped);
        }
    }
    // Short pages with a next link still consume a finite request budget.
    // Never claim completeness when the upstream page chain remains open.
    Ok(true)
}
