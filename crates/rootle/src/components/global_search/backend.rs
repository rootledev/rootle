//! Real backend (plans/0002 §4, milestones 2–3). Runs on a worker
//! thread; everything here is pure I/O → RawHit, styling happens on
//! the UI thread.

use super::grammar;
use super::model::{RawHit, SearchKind};
use rootle_provider::Provider;

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
) -> rootle_provider::ProviderResult<SearchOutcome> {
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

mod file_find;
mod grep;
mod previews;
pub(crate) use file_find::file_find_score;
use file_find::tree_file_find;
use grep::code_search;
use previews::add_blob_heads;
pub(crate) use previews::{locate_in_blob, locate_matches};

#[cfg(test)]
mod tests;
