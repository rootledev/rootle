//! Worker spawners + result styling for `App`: every provider call
//! runs on a dedicated thread and reports back over the event channel
//! (PLAN.md §6). Same `impl App` as `mod.rs` — a file split, not a
//! design split.

use super::{App, trace};
use crate::components::global_search::SearchKind;
use crate::event::AppEvent;
use crate::provider::{ErrorKind, ProviderError, ProviderResult};

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

    pub(super) fn spawn_view_search(
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

    /// Expand org marks to their repos off the UI thread, then the
    /// wizard opens with the combined list. v1.4: expanded repos keep
    /// their listing metadata; direct selections stay bare names.
    pub(super) fn spawn_expand_clone(&self, repos: Vec<String>, orgs: Vec<String>) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut repos: Vec<crate::provider::RepoInfo> = repos
                .into_iter()
                .map(crate::provider::RepoInfo::bare)
                .collect();
            let mut errors = Vec::new();
            for org in orgs {
                match provider.org_repos(&org) {
                    Ok(metas) => {
                        for m in metas {
                            let full = format!("{org}/{}", m.name);
                            let meta = crate::provider::RepoInfo {
                                name: full.clone(),
                                ..m
                            };
                            // The listing copy carries metadata — it
                            // wins over a bare selection of the same
                            // repo.
                            match repos.iter_mut().find(|r| r.name == full) {
                                Some(slot) => *slot = meta,
                                None => repos.push(meta),
                            }
                        }
                    }
                    Err(e) => errors.push(format!("{org}: {e}")),
                }
            }
            let _ = tx.send(AppEvent::CloneExpanded { repos, errors });
        });
    }

    /// Sequential clones on one worker: git is bandwidth-bound anyway,
    /// and per-repo outcomes aggregate into one CloneDone toast.
    pub(super) fn spawn_clones(&self, repos: Vec<String>, dest: std::path::PathBuf) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut ok = Vec::new();
            let mut failed = Vec::new();
            for repo in repos {
                trace(&format!("clone start {repo}"));
                let outcome = provider
                    .clone_url(&repo)
                    .map_err(|e| e.to_string())
                    .and_then(|url| {
                        // dest/org/repo — the org level avoids collisions.
                        let target = dest.join(&repo);
                        if target.exists() {
                            return Err("destination exists".into());
                        }
                        std::fs::create_dir_all(target.parent().unwrap_or(&dest))
                            .map_err(|e| e.to_string())?;
                        std::process::Command::new("git")
                            .args(["clone", &url])
                            .arg(&target)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status()
                            .map_err(|e| e.to_string())
                            .and_then(|s| {
                                if s.success() {
                                    Ok(())
                                } else {
                                    Err("git clone failed".into())
                                }
                            })
                    });
                match outcome {
                    Ok(()) => {
                        trace(&format!("clone ok {repo}"));
                        ok.push(repo);
                    }
                    Err(e) => {
                        trace(&format!("clone ERR {repo} {e}"));
                        failed.push((repo, e));
                    }
                }
            }
            let _ = tx.send(AppEvent::CloneDone { ok, failed });
        });
    }

    pub(super) fn spawn_blob(&self, sha: String, name: String) {
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

    pub(super) fn spawn_search(&self, gen_id: u64) {
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
    pub(super) fn spawn_hit_context(
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

    pub(super) fn spawn_org_repos(&self, org: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match provider.org_repos(&org) {
                Ok(repos) => AppEvent::OrgReposLoaded { org, repos },
                Err(error) => AppEvent::OrgReposFailed { org, error },
            };
            let _ = tx.send(event);
        });
    }

    pub(super) fn spawn_tree(&self, owner: String, name: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            trace(&format!("tree start {owner}/{name}"));
            let event = match provider.fetch_tree(&format!("{owner}/{name}")) {
                Ok(tree) => {
                    trace(&format!(
                        "tree ok {owner}/{name} entries={} truncated={}",
                        tree.entries.len(),
                        tree.truncated
                    ));
                    AppEvent::TreeLoaded {
                        owner,
                        name,
                        entries: tree.entries,
                        truncated: tree.truncated,
                        branch: tree.branch,
                    }
                }
                Err(error) => {
                    trace(&format!("tree ERR {owner}/{name} {error}"));
                    AppEvent::TreeFailed { owner, name, error }
                }
            };
            let _ = tx.send(event);
        });
    }

    /// Clone candidates (plans/0004 §2): (immediate repos, orgs to
    /// expand on a worker — the UI thread never calls the provider).
    /// Repo marks stay verbatim, file/dir marks fold up to the open
    /// repo, org marks fan out to ALL the org's repos.
    pub(super) fn clone_candidates(&self) -> (Vec<String>, Vec<String>) {
        let mut repos: Vec<String> = Vec::new();
        let mut orgs: Vec<String> = Vec::new();
        let marks = self.browser.visual_marks();
        if !marks.is_empty() {
            for mark in marks {
                let (title, name) = mark.split_once('/').unwrap_or(("", &mark));
                match title {
                    "orgs" => orgs.push(name.to_string()),
                    _ if Some(title) == self.browser.selected_org().as_deref() => {
                        if !repos.contains(&mark) {
                            repos.push(mark.clone());
                        }
                    }
                    _ => {
                        if let Some((owner, repo)) = self.browser.repo_coords() {
                            let full = format!("{owner}/{repo}");
                            if !repos.contains(&full) {
                                repos.push(full);
                            }
                        }
                    }
                }
            }
        } else {
            // No marks: everything in the org's repos level.
            for full in self.browser.org_repo_full_names() {
                if !repos.contains(&full) {
                    repos.push(full);
                }
            }
        }
        repos.sort();
        repos.dedup();
        orgs.sort();
        orgs.dedup();
        (repos, orgs)
    }
}
