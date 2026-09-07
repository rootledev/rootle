//! Commit history and commit-detail retrieval.

use super::{API, GitHubClient, urlencoding};
use rootle_provider::ProviderResult;

impl GitHubClient {
    /// Commit log newest-first. `limit+1` probing decides `truncated`
    /// without parsing Link headers; a hard cap of 100 keeps it to one
    /// call (the spec's ~N reading).
    pub fn log(
        &self,
        owner: &str,
        repo: &str,
        path: Option<&str>,
        ref_: Option<&str>,
        limit: Option<usize>,
    ) -> ProviderResult<(Vec<rootle_provider::LogEntry>, bool)> {
        #[derive(serde::Deserialize)]
        struct CommitItem {
            sha: String,
            commit: CommitDetail,
        }
        #[derive(serde::Deserialize)]
        struct CommitDetail {
            message: String,
            author: CommitAuthor,
        }
        #[derive(serde::Deserialize)]
        struct CommitAuthor {
            name: String,
            date: String,
        }
        let want = limit.unwrap_or(50).min(99);
        let mut url = format!("{API}/repos/{owner}/{repo}/commits?per_page={}", want + 1);
        if let Some(p) = path {
            url.push_str(&format!("&path={}", urlencoding(p)));
        }
        if let Some(r) = ref_ {
            url.push_str(&format!("&sha={}", urlencoding(r)));
        }
        let mut items: Vec<CommitItem> = self.get(&url)?;
        let truncated = items.len() > want;
        items.truncate(want);
        Ok((
            items
                .into_iter()
                .map(|c| rootle_provider::LogEntry {
                    sha: c.sha,
                    subject: c.commit.message.lines().next().unwrap_or("").to_string(),
                    author: c.commit.author.name,
                    date: c.commit.author.date,
                })
                .collect(),
            truncated,
        ))
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
