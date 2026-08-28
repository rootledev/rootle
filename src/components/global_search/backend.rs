//! Real backend (plans/0002 §4, milestones 2–3). Runs on a worker
//! thread; everything here is pure I/O → RawHit, styling happens on
//! the UI thread.

use super::grammar;
use super::model::{RawHit, SearchKind};
use crate::provider::Provider;

/// How many hits get a blob-located preview (fetch cost; rest render
/// as bare paths). Cache-first, so repeat searches are free. A budget
/// across the whole stream (v1.3), not per batch.
const PREVIEW_CAP: usize = 8;
/// Safety ceiling for locally-scored sets (repo tree file find); the
/// view keeps its own render cap.
const BACKEND_CAP: usize = 500;

/// Search outcome metadata: the clipped flag plus v1.3 index
/// freshness (indexed backends say when their index was built — a
/// lagging index is worth a badge next to the results).
#[derive(Debug, Clone, Default)]
pub struct SearchOutcome {
    pub clipped: bool,
    pub index_as_of: Option<String>,
    /// Hits the client-side grammar filter removed (plans/0012 M1) —
    /// the title's `filtered` chip.
    pub client_filtered: usize,
    /// Grammar tokens rootle couldn't express anywhere — the title's
    /// `unfiltered` chip.
    pub unfiltered: Vec<String>,
}

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
) -> crate::provider::ProviderResult<SearchOutcome> {
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
) -> crate::provider::ProviderResult<SearchOutcome> {
    let tree = provider.fetch_tree(repo_full, None)?;
    let branch = tree.branch;
    // v1.2 grammar (plans/0012 M1): quoted literals are one needle,
    // negation subtracts, language:/extension: filter by extension.
    let g = grammar::parse(query);
    let needles: Vec<String> = g.terms.iter().map(|t| t.to_lowercase()).collect();
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
        if let Some(inline) = &g.extension
            && !path_lower.ends_with(&format!(".{}", inline.to_lowercase()))
        {
            continue;
        }
        if let Some(false) = grammar::lang_matches(&g.language, &path_lower) {
            continue;
        }
        if let Some(true) = grammar::lang_matches(&g.negated_language, &path_lower) {
            continue;
        }
        if g.negated.iter().any(|n| path_lower.contains(n)) {
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
    Ok(SearchOutcome {
        clipped: client_capped,
        index_as_of: None,
        client_filtered: 0,
        unfiltered: grammar::unexpressible(&g),
    })
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
) -> crate::provider::ProviderResult<SearchOutcome> {
    let q = code_query(kind, query, scope_label, extension);
    // plans/0012 M1: the raw query goes out verbatim (GitHub's grammar
    // is a superset natively; adapters translate what they can) — and
    // the client-side subtraction filter is the no-op-safe net for
    // backends that can't express negation or language:. What rootle
    // can't express anywhere lands on the title's unfiltered chip.
    let g = grammar::parse(query);
    let unfiltered = grammar::unexpressible(&g);
    let client_filtered = std::sync::atomic::AtomicUsize::new(0);
    let preview_budget = std::sync::atomic::AtomicUsize::new(PREVIEW_CAP);
    let delivered = std::sync::atomic::AtomicUsize::new(0);
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
                    // v1.3: a provider-known line is the anchor; locating
                    // refines it (and fills the preview) when it runs.
                    line: item.line.unwrap_or(1),
                    preview: vec![],
                    match_count: needles.len() as u32,
                    stale: !item.located,
                };
                // Grep: real line numbers come from locating the matched
                if kind == SearchKind::Grep
                    && !needles.is_empty()
                    && preview_budget.load(std::sync::atomic::Ordering::Relaxed) > 0
                    && let Some((line, preview, count)) =
                        locate_matches(provider, &hit.repo, &hit.sha, &needles)
                {
                    hit.line = line;
                    hit.preview = preview;
                    hit.match_count = count;
                    hit.stale = false; // located client-side: self-healed
                    preview_budget.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                }
                batch.push(hit);
            }
            if kind == SearchKind::FileFind {
                add_blob_heads(provider, &mut batch);
            }
            let (batch, dropped) = grammar::filter_hits(&g, batch);
            client_filtered.fetch_add(dropped, std::sync::atomic::Ordering::Relaxed);
            delivered.fetch_add(batch.len(), std::sync::atomic::Ordering::Relaxed);
            on_hits(batch);
        })?;
    // The index can lie by omission: GitHub's code search doesn't
    // cover young/low-activity repos, and a scoped grep there returns
    // a silent zero. The tree can't lie — fall back to grepping the
    // default branch's tarball locally (one download, real line
    // numbers, blob shas that the API still honors).
    if kind == SearchKind::Grep
        && delivered.load(std::sync::atomic::Ordering::Relaxed) == 0
        && let Some(repo) = scope_label.strip_prefix("repo:")
        && let Some(hits) = tarball_grep(provider, repo, &g)
    {
        on_hits(hits);
    }
    Ok(SearchOutcome {
        clipped: result.truncated,
        index_as_of: result.index_as_of,
        client_filtered: client_filtered.load(std::sync::atomic::Ordering::Relaxed),
        unfiltered,
    })
}

/// The local-grep fallback (the index can't be trusted for a silent
/// zero): download the default branch's tarball, walk it, and match
/// files the way GitHub's code search would — every term present
/// somewhere in the file, negation and language/extension filters
/// applied — with previews from `locate_in_blob` and git blob shas
/// (so yank/edit/fetch-by-sha all keep working). `None` = the
/// provider can't serve a tarball (or it's over budget); the zero
/// stands.
fn tarball_grep(
    provider: &dyn Provider,
    repo_full: &str,
    g: &grammar::Grammar,
) -> Option<Vec<RawHit>> {
    const FILE_CAP: usize = 1 << 20; // matches the preview pane's blob cap
    let tarball = provider.source_tarball(repo_full).ok()?;
    let branch = provider
        .fetch_tree(repo_full, None)
        .map(|t| t.branch)
        .unwrap_or_default();
    let needles: Vec<String> = g.terms.iter().map(|t| t.to_lowercase()).collect();
    if needles.is_empty() {
        return None;
    }
    let mut hits: Vec<RawHit> = Vec::new();
    let decoder = flate2::read::GzDecoder::new(&tarball[..]);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive.entries().ok()?;
    for mut entry in entries.flatten() {
        if entry.header().entry_type() != tar::EntryType::Regular || entry.size() > FILE_CAP as u64
        {
            continue;
        }
        let path = match entry
            .path()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string))
        {
            Some(p) => p,
            None => continue,
        };
        let Some((_, path)) = path.split_once('/') else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let path_lower = path.to_lowercase();
        if let Some(inline) = &g.extension
            && !path_lower.ends_with(&format!(".{}", inline.to_lowercase()))
        {
            continue;
        }
        if let Some(false) = grammar::lang_matches(&g.language, &path_lower) {
            continue;
        }
        if let Some(true) = grammar::lang_matches(&g.negated_language, &path_lower) {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        if std::io::Read::read_to_end(&mut entry, &mut bytes).is_err() {
            continue;
        }
        if crate::sanitize::is_binary(&bytes) {
            continue;
        }
        let text = crate::sanitize::sanitize(&bytes);
        let text_lower = text.to_lowercase();
        // GitHub semantics: every term occurs somewhere in the file.
        if !needles.iter().all(|n| text_lower.contains(n)) {
            continue;
        }
        if g.negated.iter().any(|n| text_lower.contains(n)) {
            continue;
        }
        let Some((line, preview, count)) = locate_in_blob(&bytes, &needles) else {
            continue;
        };
        let sha = {
            use sha1::{Digest, Sha1};
            let mut h = Sha1::new();
            h.update(format!("blob {}\0", bytes.len()));
            h.update(&bytes);
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        hits.push(RawHit {
            repo: repo_full.to_string(),
            path: path.to_string(),
            sha,
            branch: branch.clone(),
            line,
            preview,
            match_count: count,
            stale: false,
        });
        if hits.len() >= BACKEND_CAP {
            break;
        }
    }
    Some(hits)
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

    /// The index can lie by omission (young repos aren't in GitHub's
    /// code index): a scoped grep that the API answers with a silent
    /// zero falls back to grepping the tarball locally — GitHub AND
    /// semantics, negation, real line numbers, git blob shas.
    #[test]
    fn scoped_grep_falls_back_to_tarball_on_silent_zero() {
        use crate::provider::{
            Capabilities, Provider, ProviderResult, SearchCodeResult, TreeResult,
        };

        let a_rs = b"fn target() {}\n";
        let b_rs = b"nothing relevant here\n";
        let c_rs = b"target\nand target again\n";
        let bin = vec![0u8, 159, 146, 150, 0, 1, 2, 3];

        // codeload shape: every path under "owner-repo-sha/".
        let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(enc);
        let mut add = |path: &str, bytes: &[u8]| {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("o-r-deadbeef/{path}"), bytes)
                .unwrap();
        };
        add("src/a.rs", a_rs);
        add("src/b.rs", b_rs);
        add("src/c.rs", c_rs);
        add("img.bin", &bin);
        let tarball = builder.into_inner().unwrap().finish().unwrap();

        struct Mock(Vec<u8>);
        impl Provider for Mock {
            fn name(&self) -> &str {
                "mock"
            }
            fn capabilities(&self) -> Capabilities {
                Capabilities {
                    orgs: false,
                    code_search: true,
                    file_search: true,
                    refs: false,
                    log: false,
                    blame: false,
                }
            }
            fn search(&self, _: &str) -> ProviderResult<Vec<crate::provider::SearchItem>> {
                Err("mock".into())
            }
            fn org_repos(&self, _: &str) -> ProviderResult<Vec<crate::provider::RepoInfo>> {
                Err("mock".into())
            }
            fn fetch_tree(&self, _: &str, _: Option<&str>) -> ProviderResult<TreeResult> {
                Ok(TreeResult {
                    entries: Vec::new(),
                    truncated: false,
                    branch: "main".into(),
                })
            }
            fn fetch_blob(&self, _: &str, _: &str) -> ProviderResult<Vec<u8>> {
                Err("mock".into())
            }
            fn search_code(&self, _: &str) -> ProviderResult<SearchCodeResult> {
                // The silent zero: total index omission, no error.
                Ok(SearchCodeResult {
                    hits: Vec::new(),
                    truncated: false,
                    index_as_of: None,
                })
            }
            fn clone_url(&self, _: &str) -> ProviderResult<String> {
                Err("mock".into())
            }
            fn web_url(
                &self,
                _: &str,
                _: &str,
                _: &str,
                _: Option<u32>,
                _: Option<u32>,
                _: bool,
            ) -> ProviderResult<String> {
                Err("mock".into())
            }
            fn org_url(&self, _: &str) -> ProviderResult<String> {
                Err("mock".into())
            }
            fn source_tarball(&self, _: &str) -> ProviderResult<Vec<u8>> {
                Ok(self.0.clone())
            }
        }

        let provider = Mock(tarball);
        let run = |query: &str| -> Vec<RawHit> {
            let hits = std::sync::Mutex::new(Vec::new());
            run_view_search(
                &provider,
                SearchKind::Grep,
                query,
                "repo:o/r",
                "",
                &|batch: Vec<RawHit>| hits.lock().unwrap().extend(batch),
            )
            .unwrap();
            hits.into_inner().unwrap()
        };

        let hits = run("target");
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["src/a.rs", "src/c.rs"],
            "binary skipped, non-matches skipped"
        );
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].match_count, 1, "a.rs has one matching line");
        assert_eq!(hits[1].match_count, 2, "c.rs has two");
        assert_eq!(hits[0].branch, "main");
        // git blob sha — what fetch_blob/yank/edit address.
        let want = {
            use sha1::{Digest, Sha1};
            let mut h = Sha1::new();
            h.update(format!("blob {}\0", a_rs.len()));
            h.update(a_rs);
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert_eq!(hits[0].sha, want);
        assert!(!hits[0].stale);
        assert!(
            hits[0].preview.iter().any(|(_, l)| l.contains("target")),
            "preview carries the matched line"
        );

        // GitHub AND semantics: both terms must occur in the file.
        let hits = run("target again");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/c.rs");

        // Negation subtracts.
        let hits = run("target -again");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/a.rs");
    }
}
