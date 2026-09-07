//! Bounded commit-history approximation used when GitHub has no blame endpoint.

use super::{API, GitHubClient, urlencoding};
use rootle_provider::ProviderResult;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct BlameCacheKey {
    repository: rootle_provider::RepoId,
    path: rootle_provider::RepoPath,
    revision: rootle_provider::GitRef,
}

impl GitHubClient {
    /// Blame, derived client-side: GitHub's GraphQL `Blob.blame` was
    /// removed from the schema (upstream drift, caught live
    /// 2026-08-28) and REST has none — so walk the file's commit
    /// history newest-first and claim each commit's ADDED hunks (the
    /// classic no-API approximation; intra-hunk moves and pre-window
    /// history are not distinguished). Bounded to `COMMITS` commit
    /// details — older lines carry no margin, honestly. Cached per
    /// (repo, path, ref) for the session.
    pub fn blame(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        ref_: Option<&str>,
    ) -> ProviderResult<Vec<rootle_provider::BlameRange>> {
        const COMMITS: usize = 10;
        let ref_ = ref_.unwrap_or("HEAD");
        let key = BlameCacheKey {
            repository: format!("{owner}/{repo}").into(),
            path: path.into(),
            revision: ref_.into(),
        };
        if let Some(hit) = self.blame_cache.lock().unwrap().get(&key) {
            return Ok(hit.clone());
        }

        // Current line count — the blob at the ref.
        let (bytes, _) = self.blob_at(owner, repo, path, Some(ref_))?;
        let lines_n = String::from_utf8_lossy(&bytes).lines().count() as u32;
        if lines_n == 0 {
            return Ok(Vec::new());
        }

        // Commits touching the path, newest first.
        #[derive(serde::Deserialize)]
        struct CommitListItem {
            sha: String,
            commit: CommitMeta,
        }
        #[derive(serde::Deserialize)]
        struct CommitMeta {
            author: Option<CommitAuthor>,
        }
        #[derive(serde::Deserialize, Clone)]
        struct CommitAuthor {
            name: Option<String>,
            date: Option<String>,
        }
        let url = format!(
            "{API}/repos/{owner}/{repo}/commits?path={}&sha={ref_}&per_page={COMMITS}",
            urlencoding(path)
        );
        let commits: Vec<CommitListItem> = self.get(&url)?;

        // Fetch the commit details in parallel (one bounded fan-out
        // per blame toggle — sequential round trips made the lens
        // take half a minute), then walk newest-first claiming each
        // commit's added hunks' new-line ranges.
        let urls: Vec<String> = commits
            .iter()
            .map(|c| format!("{API}/repos/{owner}/{repo}/commits/{}", c.sha))
            .collect();
        let http = self.http.clone();
        let token = self.token.clone();
        let details: Vec<Option<BlameCommitFiles>> = std::thread::scope(|s| {
            let handles: Vec<_> = urls
                .iter()
                .map(|u| {
                    let http = http.clone();
                    let token = token.clone();
                    let u = u.clone();
                    s.spawn(move || -> Option<BlameCommitFiles> {
                        let mut req = http.get(&u);
                        if let Some(t) = &token {
                            req = req.bearer_auth(t);
                        }
                        let resp = req.send().ok()?;
                        if !resp.status().is_success() {
                            return None;
                        }
                        resp.json::<BlameCommitFiles>().ok()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().ok().flatten())
                .collect()
        });
        let mut lines: Vec<Option<usize>> = vec![None; lines_n as usize]; // index into commits
        for (ci, detail) in details.into_iter().enumerate() {
            let Some(file) = detail.and_then(|d| d.files.into_iter().find(|f| f.filename == path))
            else {
                continue; // fetch failed, renamed away, or truncated
            };
            let Some(patch) = &file.patch else {
                continue; // diff truncated past GitHub's limit
            };
            for (start, end) in hunk_new_ranges(patch) {
                let (start, end) = (start.max(1), end.min(lines_n));
                if start > end {
                    continue;
                }
                for line in &mut lines[(start - 1) as usize..end as usize] {
                    if line.is_none() {
                        *line = Some(ci);
                    }
                }
            }
        }

        // Coalesce into runs.
        let mut ranges: Vec<rootle_provider::BlameRange> = Vec::new();
        let mut i = 0usize;
        while i < lines.len() {
            let Some(ci) = lines[i] else {
                i += 1;
                continue;
            };
            let start = i + 1;
            while i < lines.len() && lines[i] == Some(ci) {
                i += 1;
            }
            let c = &commits[ci];
            ranges.push(rootle_provider::BlameRange {
                start_line: start as u32,
                end_line: i as u32,
                sha: c.sha.clone(),
                author: c
                    .commit
                    .author
                    .as_ref()
                    .and_then(|a| a.name.clone())
                    .unwrap_or_default(),
                date: c
                    .commit
                    .author
                    .as_ref()
                    .and_then(|a| a.date.clone())
                    .unwrap_or_default(),
            });
        }
        self.blame_cache.lock().unwrap().insert(key, ranges.clone());
        Ok(ranges)
    }
}

#[derive(serde::Deserialize)]
struct BlameCommitFiles {
    files: Vec<CommitFile>,
}

#[derive(serde::Deserialize)]
struct CommitFile {
    filename: String,
    /// Absent when GitHub truncates very large diffs.
    patch: Option<String>,
}

/// `@@ -a,b +c,d @@` → the NEW-side (c..c+d-1) inclusive ranges of a
/// unified diff, one per hunk header.
fn hunk_new_ranges(patch: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for line in patch.lines() {
        let Some(rest) = line.strip_prefix("@@ ") else {
            continue;
        };
        // rest: "-a,b +c,d @@ …" — the new side rides after the
        // first space.
        let Some((_, new)) = rest.split_once(' ') else {
            continue;
        };
        let Some(new) = new.strip_prefix('+') else {
            continue;
        };
        // "+c,d @@" — the range ends at the next space.
        let new = new.split(' ').next().unwrap_or(new);
        let (start, count) = match new.split_once(',') {
            Some((s, c)) => (s, c),
            None => (new, "1"),
        };
        let (Ok(start), Ok(count)) = (start.parse::<u32>(), count.parse::<u32>()) else {
            continue;
        };
        if count == 0 {
            continue;
        }
        out.push((start, start + count - 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The commits-walk blame reads hunk headers: new-side inclusive
    /// ranges, count-defaults, zero-count hunks skipped.
    #[test]
    fn hunk_headers_parse_to_new_ranges() {
        let patch = concat!(
            "@@ -1,5 +1,7 @@\n",
            " context\n",
            "@@ -10,2 +12,0 @@\n",
            " gone\n",
            "@@ -20 +25,3 @@\n",
            " more"
        );
        assert_eq!(
            hunk_new_ranges(patch),
            vec![(1, 7), (25, 27)],
            "zero-count hunks drop, single-line defaults to 1"
        );
    }
}
