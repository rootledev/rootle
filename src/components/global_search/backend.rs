//! Real backend (plans/0002 §4, milestones 2–3). Runs on a worker
//! thread; everything here is pure I/O → RawHit, styling happens on
//! the UI thread.

use super::model::{RawHit, SearchKind};
use crate::provider::Provider;

/// How many hits get a blob-located preview (fetch cost; rest render
/// as bare paths). Cache-first, so repeat searches are free. A budget
/// across the whole stream (v1.3), not per batch.
const PREVIEW_CAP: usize = 8;
/// Safety ceiling for locally-scored sets (repo tree file find); the
/// view keeps its own render cap.
const BACKEND_CAP: usize = 500;

/// Build the `/search/code` query: file find matches paths, grep
/// matches content; scope/ext map to GitHub qualifiers.
///
/// PROTOCOL SURFACE (plans/0008 §4): the qualifier strings emitted
/// here (`path:`, `repo:`, `org:`, `extension:`) are what external
/// stdio providers receive verbatim in `search/code`'s `q` — adapter
/// authors translate them to their backend's grammar, and any change
/// here is a wire change that belongs in doc/provider-protocol.md.
fn code_query(kind: SearchKind, query: &str, scope_label: &str, extension: &str) -> String {
    let mut q = match kind {
        SearchKind::Grep => query.to_string(),
        SearchKind::FileFind => format!("path:{query}"),
    };
    if scope_label != "global" {
        q.push(' ');
        q.push_str(scope_label); // "repo:o/r" / "org:x" — valid qualifiers
    }
    let ext = extension.trim_start_matches('.');
    if !ext.is_empty() {
        q.push_str(&format!(" extension:{ext}"));
    }
    q
}

/// Entry point for the view's worker (plans/0002 §4): repo-scoped file
/// find runs over the cached tree (no search-API spend); everything
/// else goes through /search/code.
pub fn run_view_search(
    provider: &dyn Provider,
    kind: SearchKind,
    query: &str,
    scope_label: &str,
    extension: &str,
    on_hits: &(dyn Fn(Vec<RawHit>) + Send + Sync),
) -> crate::provider::ProviderResult<bool> {
    if kind == SearchKind::FileFind && scope_label.starts_with("repo:") {
        return tree_file_find(
            provider,
            query,
            &scope_label["repo:".len()..],
            extension,
            on_hits,
        );
    }
    code_search(provider, kind, query, scope_label, extension, on_hits)
}

/// File find over the repo's cached recursive tree, GitHub-"go to
/// file"-style (see `file_find_score`), blob heads as previews. Zero
/// search-API calls.
fn tree_file_find(
    provider: &dyn Provider,
    query: &str,
    repo_full: &str,
    extension: &str,
    on_hits: &(dyn Fn(Vec<RawHit>) + Send + Sync),
) -> crate::provider::ProviderResult<bool> {
    let tree = provider.fetch_tree(repo_full)?;
    let branch = tree.branch;
    let needles: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let ext = extension.trim_start_matches('.').to_lowercase();
    let mut scored: Vec<(i32, RawHit)> = Vec::new();
    for entry in tree.entries {
        if entry.is_dir {
            continue;
        }
        let path_lower = entry.path.to_lowercase();
        if !ext.is_empty() && !path_lower.ends_with(&format!(".{ext}")) {
            continue;
        }
        let Some(score) = file_find_score(&path_lower, &needles) else {
            continue;
        };
        scored.push((
            score,
            RawHit {
                repo: repo_full.to_string(),
                path: entry.path,
                sha: entry.sha,
                branch: branch.clone(),
                line: 1,
                preview: vec![],
                match_count: 0,
                stale: false,
            },
        ));
    }
    // Best matches first; the stable sort keeps tree order on ties.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let client_capped = scored.len() > BACKEND_CAP;
    let mut hits: Vec<RawHit> = scored
        .into_iter()
        .take(BACKEND_CAP)
        .map(|(_, h)| h)
        .collect();
    add_blob_heads(provider, &mut hits);
    on_hits(hits);
    Ok(client_capped)
}

/// GitHub-"go-to-file"-style match (behavior verified against
/// github.com's finder): the query splits on whitespace into needles,
/// and every needle must occur in the lowercased path — contiguously
/// (substring) or, failing that, as an in-order subsequence, so
/// `urldef` matches `djangosite/urls/default.py` (url in a directory,
/// def in the file name). Directory names match like file names —
/// `/term/cargo.toml` is a hit for `term`. The returned score ranks:
/// needle in the file name (best: starting it) > needle anywhere in
/// the path > scattered subsequence; longer paths lose a little (they
/// carry more noise). `None` = no match. Empty needles match
/// everything at score 0.
pub(crate) fn file_find_score(path: &str, needles: &[String]) -> Option<i32> {
    let file = path.rsplit('/').next().unwrap_or(path);
    let mut total = 0;
    let mut any = false;
    for needle in needles {
        if needle.is_empty() {
            continue;
        }
        any = true;
        let n = needle.len() as i32;
        let score = if file.starts_with(needle.as_str()) {
            120 + n
        } else if file.contains(needle.as_str()) {
            100 + n
        } else if path.contains(needle.as_str()) {
            50 + n
        } else {
            subsequence_score(path, needle)?
        };
        total += score;
    }
    if !any {
        return Some(0);
    }
    Some(total - path.len() as i32 / 8)
}

/// In-order subsequence score over the whole path: one point per
/// matched char, consecutive runs compound, chars at word boundaries
/// (after `/ . _ -`) get a bonus. `None` if some char never occurs
/// after the previous one.
fn subsequence_score(path: &str, needle: &str) -> Option<i32> {
    let hay: Vec<char> = path.chars().collect();
    let mut score = 0;
    let mut hi = 0;
    let mut prev: Option<usize> = None;
    let mut run = 0;
    for c in needle.chars() {
        let pos = hay[hi..].iter().position(|&h| h == c)? + hi;
        run = if prev.is_some_and(|p| pos == p + 1) {
            run + 1
        } else {
            1
        };
        let boundary = pos == 0 || matches!(hay[pos - 1], '/' | '.' | '_' | '-');
        score += 1 + run + i32::from(boundary) * 3;
        prev = Some(pos);
        hi = pos + 1;
    }
    Some(score)
}

/// /search/code for grep (content) and non-repo file find (path:) —
/// progressive (v1.3, plans/0011): every batch the provider streams is
/// converted and emitted through `on_hits` as it arrives; the return
/// value is the clipped flag only (metadata — the set lives with the
/// caller).
fn code_search(
    provider: &dyn Provider,
    kind: SearchKind,
    query: &str,
    scope_label: &str,
    extension: &str,
    on_hits: &(dyn Fn(Vec<RawHit>) + Send + Sync),
) -> crate::provider::ProviderResult<bool> {
    let q = code_query(kind, query, scope_label, extension);
    let preview_budget = std::sync::atomic::AtomicUsize::new(PREVIEW_CAP);
    let result =
        provider.search_code_progressive(&q, &|items: &[crate::provider::CodeMatch]| {
            let mut batch: Vec<RawHit> = Vec::with_capacity(items.len());
            for item in items {
                let needles = item.matches.clone();
                let mut hit = RawHit {
                    repo: item.repo.clone(),
                    path: item.path.clone(),
                    sha: item.sha.clone(),
                    branch: item.branch.clone(),
                    line: 1,
                    preview: vec![],
                    match_count: needles.len() as u32,
                    stale: !item.located,
                };
                // Grep: real line numbers come from locating the matched
                // texts in the blob (fragments carry no absolute numbers).
                if kind == SearchKind::Grep
                    && preview_budget.load(std::sync::atomic::Ordering::Relaxed) > 0
                    && let Some((line, preview, count)) =
                        locate_matches(provider, &hit.repo, &hit.sha, &needles)
                {
                    hit.line = line;
                    hit.preview = preview;
                    hit.match_count = count;
                    preview_budget.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                }
                batch.push(hit);
            }
            if kind == SearchKind::FileFind {
                add_blob_heads(provider, &mut batch);
            }
            on_hits(batch);
        })?;
    Ok(result.truncated)
}

/// (first match line, preview lines, matched-line count).
pub(crate) type LocatedPreview = (u32, Vec<(u32, String)>, u32);

/// Grep preview: fetch the blob (cache-first), find the lines matching
/// the query's needles, merge into ≤2 regions of ≤5 lines. Also used by
/// the lazy per-hit context path (plans/0006 §1) via `locate_in_blob`.
pub(crate) fn locate_matches(
    provider: &dyn Provider,
    repo: &str,
    sha: &str,
    needles: &[String],
) -> Option<LocatedPreview> {
    let bytes = provider.fetch_blob(repo, sha).ok()?;
    locate_in_blob(&bytes, needles)
}

/// Scan sanitized blob bytes for the needles and fold into ≤2 regions
/// of ≤5 lines (shared by the eager worker path and lazy per-hit
/// context, plans/0006 §1).
pub(crate) fn locate_in_blob(bytes: &[u8], needles: &[String]) -> Option<LocatedPreview> {
    if crate::sanitize::is_binary(bytes) {
        return None;
    }
    let text = crate::sanitize::sanitize(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let needles: Vec<String> = needles
        .iter()
        .map(|n| n.to_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    if needles.is_empty() {
        return None;
    }
    let matched: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let ll = l.to_lowercase();
            needles.iter().any(|n| ll.contains(n))
        })
        .map(|(i, _)| i)
        .collect();
    if matched.is_empty() {
        return None;
    }
    // Regions: matched lines with one context line each side; merge
    // when regions touch; cap 2 regions × 5 lines.
    let mut regions: Vec<(usize, usize)> = Vec::new();
    for &m in &matched {
        let (start, end) = (m.saturating_sub(1), (m + 2).min(lines.len()));
        match regions.last_mut() {
            Some((_, e)) if start <= *e => *e = end.max(*e),
            _ => regions.push((start, end)),
        }
    }
    let mut preview = Vec::new();
    for (start, end) in regions.into_iter().take(2) {
        let capped = end.min(start + 5);
        for (i, line) in lines.iter().enumerate().take(capped).skip(start) {
            preview.push(((i + 1) as u32, line.to_string()));
        }
    }
    Some(((matched[0] + 1) as u32, preview, matched.len() as u32))
}

/// File-find preview: the file's first lines from its blob.
fn add_blob_heads(provider: &dyn Provider, hits: &mut [RawHit]) {
    for hit in hits.iter_mut().take(PREVIEW_CAP) {
        let Ok(bytes) = provider.fetch_blob(&hit.repo, &hit.sha) else {
            continue;
        };
        if crate::sanitize::is_binary(&bytes) {
            continue;
        }
        let text = crate::sanitize::sanitize(&bytes);
        hit.preview = text
            .lines()
            .take(3)
            .enumerate()
            .map(|(i, l)| ((i + 1) as u32, l.to_string()))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needles(q: &str) -> Vec<String> {
        q.split_whitespace().map(str::to_string).collect()
    }

    /// GitHub-"go to file" semantics (verified against github.com):
    /// fuzzy subsequence over the full path, directory names included,
    /// filename matches ranked above path matches.
    #[test]
    fn file_find_matches_like_github_go_to_file() {
        // The canonical example: urldef → djangosite/urls/default.py
        // (url in a directory, def in the file name).
        let s = file_find_score("djangosite/urls/default.py", &needles("urldef"));
        assert!(s.is_some(), "subsequence across dir + file must match");

        // Directory names match like file names (substring).
        assert!(file_find_score("something/term/cargo.toml", &needles("term")).is_some());

        // Space-separated fragments each must match, in any spread.
        assert!(file_find_score("djangosite/urls/default.py", &needles("url def")).is_some());
        assert!(file_find_score("djangosite/urls/default.py", &needles("url zzz")).is_none());

        // In-order only: reversed chars never match.
        assert!(file_find_score("src/main.rs", &needles("msni")).is_none());
        assert!(file_find_score("src/main.rs", &needles("mrs")).is_some());

        // Empty query matches everything at 0.
        assert_eq!(file_find_score("src/main.rs", &[]), Some(0));
    }

    #[test]
    fn file_find_ranks_filename_above_path_above_scattered() {
        let starts = file_find_score("src/terminal.rs", &needles("term")).unwrap();
        let inside = file_find_score("src/myterm.rs", &needles("term")).unwrap();
        let dir = file_find_score("src/term/cargo.toml", &needles("term")).unwrap();
        let scattered = file_find_score("src/test/remote.rs", &needles("term")).unwrap();
        assert!(
            starts > inside,
            "needle starting the file name beats one inside it"
        );
        assert!(inside > dir, "file-name match beats a directory-only match");
        assert!(
            dir > scattered,
            "contiguous path match beats scattered chars"
        );
    }

    #[test]
    fn locate_in_blob_folds_regions_and_counts() {
        let text = b"line one\nmatch here\nbetween\nanother match\ntail";
        let needles = vec!["match".to_string()];
        let (line, preview, count) = locate_in_blob(text, &needles).unwrap();
        assert_eq!(line, 2);
        assert_eq!(count, 2);
        // One merged region: the two matches sit 2 lines apart with
        // one context line each side, so everything folds together.
        let nos: Vec<u32> = preview.iter().map(|(n, _)| *n).collect();
        assert_eq!(nos, vec![1, 2, 3, 4, 5]);
        // Binary blobs never locate.
        assert!(locate_in_blob(b"\x00\x01match", &needles).is_none());
    }

    #[test]
    fn code_query_maps_scope_and_extension() {
        use super::SearchKind;
        // Wire surface (plans/0008 §4): these strings reach external
        // providers verbatim.
        assert_eq!(
            code_query(SearchKind::Grep, "needle", "global", ""),
            "needle"
        );
        assert_eq!(
            code_query(SearchKind::FileFind, "main", "repo:o/r", ".rs"),
            "path:main repo:o/r extension:rs"
        );
        assert_eq!(
            code_query(SearchKind::Grep, "q", "org:x", "rs"),
            "q org:x extension:rs"
        );
    }
}
