//! Worker spawners — revision-lens workers: refs, log, blame, blob-at, last-commit band, plain blobs
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
use crate::event::AppEvent;
use rootle_provider::{GitRef, RepoId, Sha};
use rootle_trace::EventKind;
use serde_json::json;
use std::time::Instant;

impl App {
    /// 0019 polish: the preview band's last-commit fetch — one
    /// `log(path, limit=1)` call, only for log-capable providers.
    /// Ambient: no status noise, silent on failure.
    pub(crate) fn spawn_last_commit(&self, repo: String, path: String, ref_: Option<String>) {
        if !self.provider.capabilities().log {
            diagnostics::record_job_rejected(
                "last_commit",
                "no_log_capability",
                || json!({"repo": repo, "path": path}),
            );
            return;
        }
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "last_commit",
                    "repo": repo,
                    "path": path,
                    "ref": ref_,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let repo_id = RepoId::from(repo.clone());
                let ref_at = ref_.as_deref().map(GitRef::from);
                let entry = provider
                    .log(&repo_id, Some(&path), ref_at.as_ref(), Some(1))
                    .ok()
                    .and_then(|(entries, _)| entries.into_iter().next());
                rootle_trace::record_with(EventKind::JobFinished, || {
                    json!({
                        "job": "last_commit",
                        "outcome": if entry.is_some() { "ok" } else { "empty" },
                        "repo": repo,
                        "path": path,
                        "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                    })
                });
                if tx
                    .send(AppEvent::LastCommitLoaded { repo, path, entry })
                    .is_err()
                {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "last_commit", "send_failed": true}),
                    );
                }
            });
        });
    }

    /// Offline tests inject results; live calls carry the entire request
    /// identity back so reopening the same commit cannot accept old work.
    pub(crate) fn spawn_commit(&self, request: crate::request::CommitRequest) {
        if self.offline {
            diagnostics::record_job_rejected("commit", "offline", || {
                json!({
                    "repository": request.repository.as_str(),
                    "revision": request.revision.as_str(),
                    "gen": request.generation,
                })
            });
            return;
        }
        let provider = self.provider.clone();
        let sender = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "commit",
                    "repository": request.repository.as_str(),
                    "revision": request.revision.as_str(),
                    "gen": request.generation,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let detail = provider.commit(&request.repository, &request.revision);
                rootle_trace::record_with(EventKind::JobFinished, || {
                    json!({
                        "job": "commit",
                        "outcome": if detail.is_ok() { "ok" } else { "err" },
                        "repository": request.repository.as_str(),
                        "revision": request.revision.as_str(),
                        "gen": request.generation,
                        "error": match &detail {
                            Ok(_) => serde_json::Value::Null,
                            Err(error) => diagnostics::describe_error(error),
                        },
                        "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                    })
                });
                if sender
                    .send(AppEvent::CommitLoaded { request, detail })
                    .is_err()
                {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "commit", "send_failed": true}),
                    );
                }
            });
        });
    }

    /// Revision fetches (v1.5, plans/0016 M1): one worker per lens;
    /// landings are identity-checked by the UI.
    pub(crate) fn spawn_refs(&self, repo: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(
                EventKind::JobStarted,
                || json!({"job": "refs", "repo": repo}),
            );
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let event = match provider.refs(&RepoId::from(repo.clone())) {
                    Ok(refs) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "refs",
                                "outcome": "ok",
                                "repo": repo,
                                "branches": refs.branches.len(),
                                "tags": refs.tags.len(),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::RefsLoaded { repo, refs }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "refs",
                                "outcome": "err",
                                "repo": repo,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::RefsFailed { repo, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "refs", "send_failed": true}),
                    );
                }
            });
        });
    }

    pub(crate) fn spawn_log(&self, repo: String, path: String, ref_: Option<String>) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "log",
                    "repo": repo,
                    "path": path,
                    "ref": ref_,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                // The lens' render budget, per the bounded-compute
                // contract: past it, `truncated` tells the user to narrow.
                let limit = Some(rootle_provider::RENDER_BUDGET);
                let repo_id = RepoId::from(repo);
                let ref_at = ref_.as_deref().map(GitRef::from);
                let event = match provider.log(&repo_id, Some(&path), ref_at.as_ref(), limit) {
                    Ok((entries, truncated)) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "log",
                                "outcome": "ok",
                                "path": path,
                                "entries": entries.len(),
                                "truncated": truncated,
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::LogLoaded {
                            path,
                            entries,
                            truncated,
                        }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "log",
                                "outcome": "err",
                                "path": path,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::LogFailed { path, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "log", "send_failed": true}),
                    );
                }
            });
        });
    }

    pub(crate) fn spawn_blame(&self, repo: String, path: String, ref_: Option<String>) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "blame",
                    "repo": repo,
                    "path": path,
                    "ref": ref_,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let repo_id = RepoId::from(repo);
                let ref_at = ref_.as_deref().map(GitRef::from);
                let event = match provider.blame(&repo_id, &path, ref_at.as_ref()) {
                    Ok(ranges) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "blame",
                                "outcome": "ok",
                                "path": path,
                                "ranges": ranges.len(),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::BlameLoaded { path, ranges }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "blame",
                                "outcome": "err",
                                "path": path,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::BlameFailed { path, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "blame", "send_failed": true}),
                    );
                }
            });
        });
    }

    pub(crate) fn spawn_blob_at(
        &self,
        repo: String,
        path: String,
        ref_: String,
        subject: String,
        author: String,
        date: String,
    ) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "blob_at",
                    "repo": repo,
                    "path": path,
                    "ref": ref_,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let event = match provider.blob_at(
                    &RepoId::from(repo),
                    &path,
                    Some(&GitRef::from(ref_.as_str())),
                ) {
                    Ok((bytes, sha)) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "blob_at",
                                "outcome": "ok",
                                "path": path,
                                "sha": sha.as_str(),
                                "bytes": bytes.len(),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::BlobAtLoaded {
                            path,
                            ref_,
                            sha: sha.as_str().to_string(),
                            bytes,
                            subject,
                            author,
                            date,
                        }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "blob_at",
                                "outcome": "err",
                                "path": path,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::BlobAtFailed { path, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "blob_at", "send_failed": true}),
                    );
                }
            });
        });
    }

    pub(crate) fn spawn_blob(&self, sha: String, name: String) {
        let Some((owner, repo)) = self.browser.repo_coords() else {
            diagnostics::record_job_rejected("blob", "no_repo_coords", || json!({"sha": sha}));
            return;
        };
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "blob",
                    "repo": format!("{owner}/{repo}"),
                    "sha": sha,
                    "path": name,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let event = match fetch_blob_capped(
                    provider.as_ref(),
                    &RepoId::from(format!("{owner}/{repo}")),
                    &Sha::from(sha.as_str()),
                ) {
                    Ok(bytes) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "blob",
                                "outcome": "ok",
                                "sha": sha,
                                "bytes": bytes.len(),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::BlobLoaded { sha, name, bytes }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "blob",
                                "outcome": "err",
                                "sha": sha,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::BlobFailed { sha, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "blob", "send_failed": true}),
                    );
                }
            });
        });
    }
}
