//! Worker spawners — revision-lens workers: refs, log, blame, blob-at, last-commit band, plain blobs
//! (moved from app/workers.rs, plans/0021 M2 — a pure move).

use super::{App, fetch_blob_capped, trace};
use crate::event::AppEvent;
use rootle_provider::{GitRef, RepoId, Sha};

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
            let repo_id = RepoId::from(repo.clone());
            let ref_at = ref_.as_deref().map(GitRef::from);
            let entry = provider
                .log(&repo_id, Some(&path), ref_at.as_ref(), Some(1))
                .ok()
                .and_then(|(entries, _)| entries.into_iter().next());
            let _ = tx.send(AppEvent::LastCommitLoaded { repo, path, entry });
        });
    }

    /// Offline tests inject results; live calls carry the entire request
    /// identity back so reopening the same commit cannot accept old work.
    pub(crate) fn spawn_commit(&self, request: crate::request::CommitRequest) {
        if self.offline {
            return;
        }
        let provider = self.provider.clone();
        let sender = self.tx.clone();
        std::thread::spawn(move || {
            let detail = provider.commit(&request.repository, &request.revision);
            let _ = sender.send(AppEvent::CommitLoaded { request, detail });
        });
    }

    /// Revision fetches (v1.5, plans/0016 M1): one worker per lens;
    /// landings are identity-checked by the UI.
    pub(crate) fn spawn_refs(&self, repo: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match provider.refs(&RepoId::from(repo.clone())) {
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
            let limit = Some(rootle_provider::RENDER_BUDGET);
            let repo_id = RepoId::from(repo);
            let ref_at = ref_.as_deref().map(GitRef::from);
            let event = match provider.log(&repo_id, Some(&path), ref_at.as_ref(), limit) {
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
            let repo_id = RepoId::from(repo);
            let ref_at = ref_.as_deref().map(GitRef::from);
            let event = match provider.blame(&repo_id, &path, ref_at.as_ref()) {
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
            let event = match provider.blob_at(
                &RepoId::from(repo),
                &path,
                Some(&GitRef::from(ref_.as_str())),
            ) {
                Ok((bytes, sha)) => AppEvent::BlobAtLoaded {
                    path,
                    ref_,
                    sha: sha.as_str().to_string(),
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
            let event = match fetch_blob_capped(
                provider.as_ref(),
                &RepoId::from(format!("{owner}/{repo}")),
                &Sha::from(sha.as_str()),
            ) {
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
