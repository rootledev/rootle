use super::{BACKEND_CAP, Provider, RawHit, SearchOutcome, add_blob_heads, grammar};

/// File find over the repo's cached recursive tree, GitHub-"go to
/// file"-style (see `file_find_score`), blob heads as previews. Zero
/// search-API calls.
pub(super) fn tree_file_find(
    provider: &dyn Provider,
    query: &str,
    repo_full: &str,
    extension: &str,
    on_hits: &(dyn Fn(Vec<RawHit>) + Send + Sync),
) -> rootle_provider::ProviderResult<SearchOutcome> {
    let tree = provider.fetch_tree(&rootle_provider::RepoId::from(repo_full), None)?;
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
pub(super) fn subsequence_score(path: &str, needle: &str) -> Option<i32> {
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
