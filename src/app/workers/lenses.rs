//! Worker spawners — revision-lens workers: refs, log, blame, blob-at, last-commit band, plain blobs
//! (moved from app/workers.rs, plans/0021 M2 — a pure move).

use super::{App, fetch_blob_capped, trace};
use crate::event::AppEvent;

impl App {
    /// 0019 polish: the preview band's last-commit fetch — one
    /// `log(path, limit=1)` call, only for log-capable providers.
    /// Ambient: no status noise, silent on failure.
    pub(crate) fn spawn_last_commit(&self, repo: String, path: String, ref_: Option<String>) {
        if !self.provider.capabilities().log {
            return;
        }
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let entry = provider
                .log(&repo, Some(&path), ref_.as_deref(), Some(1))
                .ok()
                .and_then(|(entries, _)| entries.into_iter().next());
            let _ = tx.send(AppEvent::LastCommitLoaded { repo, path, entry });
        });
    }

    /// Revision fetches (v1.5, plans/0016 M1): one worker per lens;
    /// landings are identity-checked by the UI.
    pub(crate) fn spawn_refs(&self, repo: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match provider.refs(&repo) {
                Ok(refs) => AppEvent::RefsLoaded { repo, refs },
                Err(error) => AppEvent::RefsFailed { repo, error },
            };
            let _ = tx.send(event);
        });
    }

    pub(crate) fn spawn_log(&self, repo: String, path: String, ref_: Option<String>) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // The lens' render budget, per the bounded-compute
            // contract: past it, `truncated` tells the user to narrow.
            let limit = Some(crate::provider::RENDER_BUDGET);
            let event = match provider.log(&repo, Some(&path), ref_.as_deref(), limit) {
                Ok((entries, truncated)) => AppEvent::LogLoaded {
                    path,
                    entries,
                    truncated,
                },
                Err(error) => AppEvent::LogFailed { path, error },
            };
            let _ = tx.send(event);
        });
    }

    pub(crate) fn spawn_blame(&self, repo: String, path: String, ref_: Option<String>) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match provider.blame(&repo, &path, ref_.as_deref()) {
                Ok(ranges) => AppEvent::BlameLoaded { path, ranges },
                Err(error) => AppEvent::BlameFailed { path, error },
            };
            let _ = tx.send(event);
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
        std::thread::spawn(move || {
            let event = match provider.blob_at(&repo, &path, Some(&ref_)) {
                Ok((bytes, sha)) => AppEvent::BlobAtLoaded {
                    path,
                    ref_,
                    sha,
                    bytes,
                    subject,
                    author,
                    date,
                },
                Err(error) => AppEvent::BlobAtFailed { path, error },
            };
            let _ = tx.send(event);
        });
    }

    pub(crate) fn spawn_blob(&self, sha: String, name: String) {
        let Some((owner, repo)) = self.browser.repo_coords() else {
            return;
        };
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            trace(&format!("blob start {sha}"));
            let event = match fetch_blob_capped(provider.as_ref(), &format!("{owner}/{repo}"), &sha)
            {
                Ok(bytes) => {
                    trace(&format!("blob ok {sha} {} bytes", bytes.len()));
                    AppEvent::BlobLoaded { sha, name, bytes }
                }
                Err(error) => {
                    trace(&format!("blob ERR {sha} {error}"));
                    AppEvent::BlobFailed { sha, error }
                }
            };
            let _ = tx.send(event);
        });
    }
}
