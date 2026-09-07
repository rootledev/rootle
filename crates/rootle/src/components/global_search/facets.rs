//! Facets from the stream (plans/0012 M3): filter chips computed from
//! the hits the view already holds — zero backend cost. One chip per
//! repo and one per language (extension-derived through the M1 table,
//! so a chip reads `rust` not `rs`), each carrying its hit count.
//! Selecting a chip is a committed local filter over the accumulated
//! set — the same narrowing the `/` filter applies, and the two
//! compose (facet first, then filter text).

use super::GlobalSearch;
use super::grammar::{ext_lang, lang_exts, path_ext};
use super::model::SearchHit;
use std::collections::BTreeMap;

/// Which dimension a chip filters. Chip identity is (kind, name) —
/// counts live beside it in `Facet` and move as batches land, so they
/// never take part in equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FacetKind {
    Repo,
    Lang,
}

/// A chip's identity: the committed filter `GlobalSearch` stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FacetId {
    pub kind: FacetKind,
    pub name: String,
}

impl FacetId {
    /// Does this hit belong to the facet? Repos compare exactly;
    /// language facets match the whole extension family (`rust` ≙
    /// `rs`), and a chip named for an unknown extension (`txt`)
    /// matches only itself.
    pub(super) fn matches(&self, hit: &SearchHit) -> bool {
        match self.kind {
            FacetKind::Repo => hit.repo == self.name,
            // A known language matches its whole extension family;
            // `lang_exts` (not `ext_lang`) is the name→extensions
            // direction this check needs. A chip named for an unknown
            // extension (`txt`) matches only itself.
            FacetKind::Lang => match lang_exts(&self.name) {
                Some(exts) => exts.contains(&path_ext(&hit.path).as_str()),
                None => path_ext(&hit.path) == self.name,
            },
        }
    }
}

/// One chip: identity + how many accumulated hits it counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Facet {
    pub id: FacetId,
    pub count: usize,
}

/// Chip row over an accumulated hit set: repos first, then languages,
/// each group most-hits-first with ties alphabetical. Counts always
/// cover the whole set — they don't narrow with a selection; the
/// selected chip itself is the visible source of the narrowing.
pub(super) fn facets(hits: &[SearchHit]) -> Vec<Facet> {
    let mut repos: BTreeMap<String, usize> = BTreeMap::new();
    let mut langs: BTreeMap<String, usize> = BTreeMap::new();
    for hit in hits {
        if !hit.repo.is_empty() {
            *repos.entry(hit.repo.clone()).or_insert(0) += 1;
        }
        if let Some(lang) = lang_chip(&hit.path) {
            *langs.entry(lang).or_insert(0) += 1;
        }
    }
    let chips = |map: BTreeMap<String, usize>, kind: FacetKind| {
        let mut out: Vec<Facet> = map
            .into_iter()
            .map(|(name, count)| Facet {
                id: FacetId { kind, name },
                count,
            })
            .collect();
        out.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.id.name.cmp(&b.id.name))
        });
        out
    };
    let mut out = chips(repos, FacetKind::Repo);
    out.extend(chips(langs, FacetKind::Lang));
    out
}

fn lang_chip(path: &str) -> Option<String> {
    let file = path.rsplit('/').next().unwrap_or(path);
    // Dot positions, never slices: `path_ext` lowercases, which can
    // change byte lengths — compare indices instead of cutting.
    let dot = file.rfind('.')?;
    if dot == 0 || dot + 1 >= file.len() {
        return None; // hidden file (".gitignore") or trailing dot
    }
    let ext = path_ext(file);
    Some(ext_lang(&ext).unwrap_or(ext.as_str()).to_string())
}

impl GlobalSearch {
    /// The chip row right now — computed, never stored, so counts
    /// move as batches land and a full-set replacement reshapes them.
    pub(super) fn facets(&self) -> Vec<Facet> {
        facets(&self.hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(repo: &str, path: &str) -> SearchHit {
        SearchHit::plain(repo, path, 1, vec![], 0, String::new())
    }

    #[test]
    fn chips_group_by_repo_and_language_with_counts() {
        let hits = vec![
            hit("a/one", "src/main.rs"),
            hit("a/one", "docs/x.md"),
            hit("b/two", "src/lib.rs"),
            hit("b/two", "notes.txt"),
        ];
        let chips = facets(&hits);
        let names: Vec<(FacetKind, &str, usize)> = chips
            .iter()
            .map(|c| (c.id.kind, c.id.name.as_str(), c.count))
            .collect();
        // Repos first, then languages; counts desc, ties alphabetical.
        assert_eq!(
            names,
            vec![
                (FacetKind::Repo, "a/one", 2),
                (FacetKind::Repo, "b/two", 2),
                (FacetKind::Lang, "rust", 2),
                (FacetKind::Lang, "markdown", 1),
                (FacetKind::Lang, "txt", 1),
            ]
        );
    }

    #[test]
    fn language_facet_matches_whole_extension_family() {
        let rust = FacetId {
            kind: FacetKind::Lang,
            name: "rust".into(),
        };
        assert!(rust.matches(&hit("a/one", "src/main.rs")));
        assert!(!rust.matches(&hit("a/one", "docs/x.md")));
        // Unknown-extension chips match only themselves.
        let txt = FacetId {
            kind: FacetKind::Lang,
            name: "txt".into(),
        };
        assert!(txt.matches(&hit("a/one", "notes.txt")));
        assert!(!txt.matches(&hit("a/one", "src/main.rs")));
        // Repos compare exactly.
        let repo = FacetId {
            kind: FacetKind::Repo,
            name: "a/one".into(),
        };
        assert!(repo.matches(&hit("a/one", "src/main.rs")));
        assert!(!repo.matches(&hit("b/two", "src/main.rs")));
    }

    #[test]
    fn extension_less_paths_get_no_language_chip() {
        let chips = facets(&[hit("a/one", "Makefile"), hit("a/one", ".gitignore")]);
        // Repo chip only — "Makefile"/"gitignore" aren't languages.
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].id.kind, FacetKind::Repo);
    }
}
