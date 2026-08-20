//! Worker spawners + result styling for `App`: every provider call
//! runs on a dedicated thread and reports back over the event channel
//! (PLAN.md §6). Same `impl App` as `mod.rs` — a file split, not a
//! design split.

use super::{App, trace};
use crate::components::global_search::SearchKind;
use crate::event::AppEvent;

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
            let event = match crate::components::global_search::run_view_search(
                provider.as_ref(),
                kind,
                &query,
                &scope,
                &extension,
            ) {
                Ok(hits) => {
                    trace(&format!("view search ok gen={gen_id} hits={}", hits.len()));
                    AppEvent::GlobalSearchResults { gen_id, hits }
                }
                Err(message) => {
                    trace(&format!("view search ERR gen={gen_id} {message}"));
                    AppEvent::GlobalSearchFailed { gen_id, message }
                }
            };
            let _ = tx.send(event);
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
                let outcome = provider.clone_url(&repo).and_then(|url| {
                    let name = repo.rsplit('/').next().unwrap_or(&repo);
                    let target = dest.join(name);
                    if target.exists() {
                        return Err("destination exists".into());
                    }
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
            let event = match provider.fetch_blob(&format!("{owner}/{repo}"), &sha) {
                Ok(bytes) => {
                    trace(&format!("blob ok {sha} {} bytes", bytes.len()));
                    AppEvent::BlobLoaded { sha, name, bytes }
                }
                Err(message) => {
                    trace(&format!("blob ERR {sha} {message}"));
                    AppEvent::BlobFailed { sha, message }
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
                Err(message) => {
                    trace(&format!("search ERR gen={gen_id} {message}"));
                    AppEvent::SearchFailed { gen_id, message }
                }
            };
            let _ = tx.send(event);
            trace(&format!("search sent gen={gen_id}"));
        });
    }

    pub(super) fn spawn_org_repos(&self, org: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match provider.org_repos(&org) {
                Ok(repos) => AppEvent::OrgReposLoaded { org, repos },
                Err(message) => AppEvent::OrgReposFailed { org, message },
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
                Err(message) => {
                    trace(&format!("tree ERR {owner}/{name} {message}"));
                    AppEvent::TreeFailed {
                        owner,
                        name,
                        message,
                    }
                }
            };
            let _ = tx.send(event);
        });
    }

    /// Repos for the clone wizard (plans/0004 §2): VISUAL marks resolve
    /// to their repos (file/dir marks fold up to the open repo); no
    /// marks → the repos level of the selected org.
    pub(super) fn clone_candidates(&self) -> Vec<String> {
        fn push(repos: &mut Vec<String>, r: String) {
            if !repos.contains(&r) {
                repos.push(r);
            }
        }
        let mut repos: Vec<String> = Vec::new();
        let marks = self.browser.visual_marks();
        if !marks.is_empty() {
            for mark in marks {
                let (title, _name) = mark.split_once('/').unwrap_or(("", &mark));
                if Some(title) == self.browser.selected_org().as_deref() {
                    push(&mut repos, mark.clone()); // repo-level mark: "org/repo"
                } else if let Some((owner, repo)) = self.browser.repo_coords() {
                    push(&mut repos, format!("{owner}/{repo}")); // file/dir → its repo
                }
            }
        } else {
            // No marks: everything in the org's repos level.
            for full in self.browser.org_repo_full_names() {
                push(&mut repos, full);
            }
        }
        repos
    }
}
