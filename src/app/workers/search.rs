//! Worker spawners — global-search workers: view searches, per-hit context, whole-file fetches
//! (moved from app/workers.rs, plans/0021 M2 — a pure move).

use super::{App, fetch_blob_capped, trace};
use crate::components::global_search::SearchKind;
use crate::event::AppEvent;

impl App {
    pub(crate) fn spawn_view_search(
        &self,
        gen_id: u64,
        kind: SearchKind,
        query: String,
        scope: String,
        extension: String,
    ) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            trace(&format!(
                "view search start gen={gen_id} q={query:?} scope={scope}"
            ));
            // Streamed batches go straight to the event loop as they
            // arrive (v1.3, plans/0011) — the worker stays blocked in
            // the provider call until the final metadata reply. The
            // sender is not Sync, so the sink holds it through a mutex
            // (one lock per batch).
            let sink_tx = std::sync::Mutex::new(tx.clone());
            let on_hits = move |hits: Vec<crate::components::global_search::RawHit>| {
                if let Ok(tx) = sink_tx.lock() {
                    let _ = tx.send(AppEvent::GlobalSearchDelta { gen_id, hits });
                }
            };
            let event = match crate::components::global_search::run_view_search(
                provider.as_ref(),
                kind,
                &query,
                &scope,
                &extension,
                &on_hits,
            ) {
                Ok(outcome) => {
                    trace(&format!(
                        "view search ok gen={gen_id} clipped={} index={:?}",
                        outcome.clipped, outcome.index_as_of
                    ));
                    AppEvent::GlobalSearchResults {
                        gen_id,
                        hits: Vec::new(),
                        clipped: outcome.clipped,
                        index: outcome.index_as_of,
                        client_filtered: outcome.client_filtered,
                        unfiltered: outcome.unfiltered,
                    }
                }
                Err(error) => {
                    trace(&format!("view search ERR gen={gen_id} {error}"));
                    AppEvent::GlobalSearchFailed { gen_id, error }
                }
            };
            let _ = tx.send(event);
        });
    }

    pub(crate) fn spawn_search(&self, gen_id: u64) {
        let Some(popup) = &self.popup else { return };
        let query = popup.input.value();
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            trace(&format!("search start gen={gen_id} q={query:?}"));
            let event = match provider.search(&query) {
                Ok(items) => {
                    trace(&format!("search ok gen={gen_id} items={}", items.len()));
                    AppEvent::SearchResults { gen_id, items }
                }
                Err(error) => {
                    trace(&format!("search ERR gen={gen_id} {error}"));
                    AppEvent::SearchFailed { gen_id, error }
                }
            };
            let _ = tx.send(event);
            trace(&format!("search sent gen={gen_id}"));
        });
    }

    /// Lazy per-hit context (plans/0006 §1): fetch the selected bare
    /// hit's blob and locate the query's context. Cache-first, so the
    /// second visit of a hit is free.
    pub(crate) fn spawn_hit_context(
        &self,
        gen_id: u64,
        hit: crate::components::global_search::SearchHit,
        query: String,
    ) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let sha = hit.sha.clone();
            trace(&format!(
                "hit ctx start gen={gen_id} {} sha={sha}",
                hit.path
            ));
            let event = match fetch_blob_capped(provider.as_ref(), &hit.repo, &sha) {
                Ok(bytes) => {
                    let needles: Vec<String> =
                        query.split_whitespace().map(str::to_string).collect();
                    let located =
                        crate::components::global_search::locate_in_blob(&bytes, &needles);
                    match located {
                        Some((line, preview, count)) => {
                            trace(&format!("hit ctx ok gen={gen_id} {sha}"));
                            AppEvent::HitContextLoaded {
                                gen_id,
                                repo: hit.repo,
                                path: hit.path,
                                sha,
                                line,
                                preview,
                                match_count: count,
                                query,
                            }
                        }
                        None => {
                            // Blob fetched but nothing matched — the hit
                            // is unlocatable, not just pending (§4).
                            trace(&format!("hit ctx none gen={gen_id} {sha}"));
                            AppEvent::HitContextMissing { gen_id, sha }
                        }
                    }
                }
                Err(error) => {
                    trace(&format!("hit ctx ERR gen={gen_id} {sha} {error}"));
                    // Auth/throttle surface a status line; other kinds
                    // stay quiet (plans/0008 §2).
                    AppEvent::HitContextFailed { gen_id, sha, error }
                }
            };
            let _ = tx.send(event);
        });
    }

    /// The expanded file pane (plans/0012 M2): fetch the hit's whole
    /// blob. Cache-first — the lazy context fetch usually warmed the
    /// exact (repo, sha), so expanding a located hit is free.
    pub(crate) fn spawn_hit_file(
        &self,
        gen_id: u64,
        hit: crate::components::global_search::SearchHit,
    ) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let sha = hit.sha.clone();
            trace(&format!(
                "hit file start gen={gen_id} {} sha={sha}",
                hit.path
            ));
            let event = match fetch_blob_capped(provider.as_ref(), &hit.repo, &sha) {
                Ok(bytes) => {
                    trace(&format!(
                        "hit file ok gen={gen_id} {sha} {} bytes",
                        bytes.len()
                    ));
                    AppEvent::HitFileLoaded {
                        gen_id,
                        repo: hit.repo,
                        path: hit.path,
                        sha,
                        bytes,
                    }
                }
                Err(error) => {
                    trace(&format!("hit file ERR gen={gen_id} {sha} {error}"));
                    AppEvent::HitFileFailed { gen_id, sha, error }
                }
            };
            let _ = tx.send(event);
        });
    }
}
