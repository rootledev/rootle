//! Worker spawners — global-search workers: view searches, per-hit context, whole-file fetches
//! (moved from app/workers.rs, plans/0021 M2 — a pure move).
//!
//! 0030: every spawn records JobStarted with its semantic identity
//! under a fresh operation id; the worker body runs inside
//! `in_operation` so backend transport records inherit the
//! correlation, and records its real outcome (counts, classification,
//! duration, send failure) on exit. Producers are free while tracing
//! is disabled.

use super::{App, fetch_blob_capped};
use crate::app::diagnostics;
use crate::components::global_search::SearchKind;
use crate::event::AppEvent;
use rootle_trace::EventKind;
use serde_json::json;
use std::time::Instant;

impl App {
    pub(crate) fn spawn_view_search(
        &self,
        gen_id: crate::request::ViewGeneration,
        kind: SearchKind,
        query: String,
        scope: String,
        extension: String,
    ) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "view_search",
                    "gen": gen_id,
                    "kind": kind.slug(),
                    "query": diagnostics::text(&query),
                    "scope": scope,
                    "extension": diagnostics::text(&extension),
                })
            });
        });
        std::thread::spawn(move || {
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                // Streamed batches go straight to the event loop as they
                // arrive (v1.3, plans/0011) — the worker stays blocked in
                // the provider call until the final metadata reply. The
                // sender is not Sync, so the sink holds it through a mutex
                // (one lock per batch).
                let batches = std::sync::atomic::AtomicU64::new(0);
                let counter = &batches;
                let sink_tx = std::sync::Mutex::new(tx.clone());
                let on_hits = move |hits: Vec<crate::components::global_search::RawHit>| {
                    if rootle_trace::enabled() {
                        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    if let Ok(tx) = sink_tx.lock()
                        && tx
                            .send(AppEvent::GlobalSearchDelta { gen_id, hits })
                            .is_err()
                    {
                        rootle_trace::record_with(
                            EventKind::Error,
                            || json!({"job":"view_search_delta","gen":gen_id,"send_failed":true}),
                        );
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
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "view_search",
                                "outcome": "ok",
                                "gen": gen_id,
                                "batches": batches.load(std::sync::atomic::Ordering::Relaxed),
                                "clipped": outcome.clipped,
                                "index_as_of": outcome.index_as_of.is_some(),
                                "client_filtered": outcome.client_filtered,
                                "unfiltered": outcome.unfiltered.len(),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
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
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "view_search",
                                "outcome": "err",
                                "gen": gen_id,
                                "batches": batches.load(std::sync::atomic::Ordering::Relaxed),
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::GlobalSearchFailed { gen_id, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "view_search", "send_failed": true}),
                    );
                }
            });
        });
    }

    pub(crate) fn spawn_search(&self, gen_id: crate::request::SearchGeneration) {
        let Some(popup) = &self.popup else {
            diagnostics::record_job_rejected("search", "no_search_popup", || json!({}));
            return;
        };
        let query = popup.input.value();
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "search",
                    "gen": gen_id,
                    "query": diagnostics::text(&query),
                })
            });
        });
        std::thread::spawn(move || {
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let event = match provider.search(&query) {
                    Ok(items) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "search",
                                "outcome": "ok",
                                "gen": gen_id,
                                "items": items.len(),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::SearchResults { gen_id, items }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "search",
                                "outcome": "err",
                                "gen": gen_id,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::SearchFailed { gen_id, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "search", "send_failed": true}),
                    );
                }
            });
        });
    }

    /// Lazy per-hit context (plans/0006 §1): fetch the selected bare
    /// hit's blob and locate the query's context. Cache-first, so the
    /// second visit of a hit is free.
    pub(crate) fn spawn_hit_context(
        &self,
        gen_id: crate::request::ViewGeneration,
        hit: crate::components::global_search::SearchHit,
        query: String,
    ) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "hit_context",
                    "gen": gen_id,
                    "repo": hit.repo,
                    "path": hit.path,
                    "sha": hit.sha,
                    "query": diagnostics::text(&query),
                })
            });
        });
        std::thread::spawn(move || {
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let sha = hit.sha.clone();
                let event = match fetch_blob_capped(
                    provider.as_ref(),
                    &rootle_provider::RepoId::from(hit.repo.as_str()),
                    &rootle_provider::Sha::from(sha.as_str()),
                ) {
                    Ok(bytes) => {
                        let needles: Vec<String> =
                            query.split_whitespace().map(str::to_string).collect();
                        let located =
                            crate::components::global_search::locate_in_blob(&bytes, &needles);
                        match located {
                            Some((line, preview, count)) => {
                                rootle_trace::record_with(EventKind::JobFinished, || {
                                    json!({
                                        "job": "hit_context",
                                        "outcome": "located",
                                        "sha": sha,
                                        "line": line,
                                        "matches": count,
                                        "bytes": bytes.len(),
                                        "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                                    })
                                });
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
                                rootle_trace::record_with(EventKind::JobFinished, || {
                                    json!({
                                        "job": "hit_context",
                                        "outcome": "unlocatable",
                                        "sha": sha,
                                        "bytes": bytes.len(),
                                        "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                                    })
                                });
                                AppEvent::HitContextMissing { gen_id, sha }
                            }
                        }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "hit_context",
                                "outcome": "err",
                                "sha": sha,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        // Auth/throttle surface a status line; other kinds
                        // stay quiet (plans/0008 §2).
                        AppEvent::HitContextFailed { gen_id, sha, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "hit_context", "send_failed": true}),
                    );
                }
            });
        });
    }

    /// The expanded file pane (plans/0012 M2): fetch the hit's whole
    /// blob. Cache-first — the lazy context fetch usually warmed the
    /// exact (repo, sha), so expanding a located hit is free.
    pub(crate) fn spawn_hit_file(
        &self,
        gen_id: crate::request::ViewGeneration,
        hit: crate::components::global_search::SearchHit,
    ) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "hit_file",
                    "gen": gen_id,
                    "repo": hit.repo,
                    "path": hit.path,
                    "sha": hit.sha,
                })
            });
        });
        std::thread::spawn(move || {
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let sha = hit.sha.clone();
                let event = match fetch_blob_capped(
                    provider.as_ref(),
                    &rootle_provider::RepoId::from(hit.repo.as_str()),
                    &rootle_provider::Sha::from(sha.as_str()),
                ) {
                    Ok(bytes) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "hit_file",
                                "outcome": "ok",
                                "sha": sha,
                                "bytes": bytes.len(),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::HitFileLoaded {
                            gen_id,
                            repo: hit.repo,
                            path: hit.path,
                            sha,
                            bytes,
                        }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "hit_file",
                                "outcome": "err",
                                "sha": sha,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::HitFileFailed { gen_id, sha, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "hit_file", "send_failed": true}),
                    );
                }
            });
        });
    }
}
