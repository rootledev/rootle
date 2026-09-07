use super::{PREVIEW_CAP, Provider, RawHit};

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
    let bytes = provider
        .fetch_blob(
            &rootle_provider::RepoId::from(repo),
            &rootle_provider::Sha::from(sha),
        )
        .ok()?;
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
pub(super) fn add_blob_heads(provider: &dyn Provider, hits: &mut [RawHit]) {
    for hit in hits.iter_mut().take(PREVIEW_CAP) {
        let Ok(bytes) = provider.fetch_blob(
            &rootle_provider::RepoId::from(hit.repo.as_str()),
            &rootle_provider::Sha::from(hit.sha.as_str()),
        ) else {
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
