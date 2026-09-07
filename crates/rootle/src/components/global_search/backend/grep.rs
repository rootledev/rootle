use super::{
    BACKEND_CAP, PREVIEW_CAP, Provider, RawHit, SearchKind, SearchOutcome, add_blob_heads,
    code_query, grammar, locate_in_blob, locate_matches,
};

/// /search/code for grep (content) and non-repo file find (path:) —
/// progressive (v1.3, plans/0011): every batch the provider streams is
/// converted and emitted through `on_hits` as it arrives; the return
/// value is the clipped flag only (metadata — the set lives with the
/// caller).
pub(super) fn code_search(
    provider: &dyn Provider,
    kind: SearchKind,
    query: &str,
    scope_label: &str,
    extension: &str,
    on_hits: &(dyn Fn(Vec<RawHit>) + Send + Sync),
) -> rootle_provider::ProviderResult<SearchOutcome> {
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
        provider.search_code_progressive(&q, &|items: &[rootle_provider::CodeMatch]| {
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
pub(super) fn tarball_grep(
    provider: &dyn Provider,
    repo_full: &str,
    g: &grammar::Grammar,
) -> Option<Vec<RawHit>> {
    const FILE_CAP: usize = 1 << 20; // matches the preview pane's blob cap
    let tarball = provider
        .source_tarball(&rootle_provider::RepoId::from(repo_full))
        .ok()?;
    let branch = provider
        .fetch_tree(&rootle_provider::RepoId::from(repo_full), None)
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
