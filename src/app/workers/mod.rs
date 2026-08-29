//! Worker spawners + result styling for `App`: every provider call
//! runs on a dedicated thread and reports back over the event channel
//! (PLAN.md §6). Same `impl App` as `mod.rs` — a file split, not a
//! design split.

use super::{App, trace};
use crate::components::global_search::SearchKind;
use crate::provider::{ErrorKind, ProviderError, ProviderResult};

pub(super) mod lenses;
pub(super) mod lifecycle;
pub(super) mod search;

/// Blobs over 1 MiB never enter the app, whatever the provider: the
/// preview pane rejects them anyway, and no backend (in-tree or stdio)
/// should be able to push a giant payload through the pipe. The
/// uniform guarantee lives here, at the boundary, not in each provider
/// (plans/0009 R1).
const BLOB_CAP: usize = 1024 * 1024;

/// fetch_blob with the uniform cap; every blob path in the app goes
/// through this.
fn fetch_blob_capped(
    provider: &dyn crate::provider::Provider,
    repo: &str,
    sha: &str,
) -> ProviderResult<Vec<u8>> {
    let bytes = provider.fetch_blob(repo, sha)?;
    if bytes.len() > BLOB_CAP {
        return Err(ProviderError::new(
            ErrorKind::Provider,
            format!(
                "blob {sha} is {} KiB — over the 1 MiB preview cap",
                bytes.len() / 1024
            ),
        ));
    }
    Ok(bytes)
}

impl App {
    /// Style raw hits at the UI boundary: syntect highlight + grep
    /// match chips (plans/0002 §5). Runs on mock and real hits alike.
    pub(super) fn finish_hits(
        &self,
        hits: Vec<crate::components::global_search::SearchHit>,
        kind: SearchKind,
        query: &str,
    ) -> Vec<crate::components::global_search::SearchHit> {
        let mut hits: Vec<_> = hits
            .into_iter()
            .map(|hit| {
                let lines = self.highlighter.highlight(&hit.path, &hit.preview_text());
                hit.with_highlighted(lines)
            })
            .collect();
        if kind == SearchKind::Grep {
            crate::components::global_search::highlight_matches(
                &mut hits,
                query,
                self.theme.semantic.search_match,
                self.theme.semantic.crust,
            );
        }
        hits
    }
}
