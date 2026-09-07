//! Worker spawners — app-lifecycle workers: update check, declared-provider install, clone fan-out, org/tree loads
//! (moved from app/workers.rs, plans/0021 M2 — a pure move).
//!
//! 0030: every spawn records JobStarted with its semantic identity
//! under a fresh operation id; the worker body runs inside
//! `in_operation` so provider/child transport records inherit the
//! correlation, and records its real outcome (counts, classification,
//! duration, send failure) on exit. Producers are free while tracing
//! is disabled.

use super::App;
use crate::app::diagnostics;
use crate::event::AppEvent;
use rootle_provider::{GitRef, RepoId};
use rootle_trace::EventKind;
use serde_json::json;
use std::time::Instant;

impl App {
    /// Expand org marks to their repos off the UI thread, then the
    /// wizard opens with the combined list. v1.4: expanded repos keep
    /// their listing metadata; direct selections stay bare names.
    pub(crate) fn spawn_expand_clone(&self, repos: Vec<String>, orgs: Vec<String>) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let repo_count = repos.len();
        let org_count = orgs.len();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "expand_clone",
                    "repos": repo_count,
                    "orgs": org_count,
                    "org_names": orgs,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let mut repos: Vec<rootle_provider::RepoInfo> = repos
                    .into_iter()
                    .map(rootle_provider::RepoInfo::bare)
                    .collect();
                let mut errors = Vec::new();
                for org in orgs {
                    match provider.org_repos(&org) {
                        Ok(metas) => {
                            for m in metas {
                                let full = format!("{org}/{}", m.name);
                                let meta = rootle_provider::RepoInfo {
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
                rootle_trace::record_with(EventKind::JobFinished, || {
                    json!({
                        "job": "expand_clone",
                        "outcome": if errors.is_empty() { "ok" } else if repos.is_empty() { "err" } else { "partial" },
                        "repos": repos.len(),
                        "org_errors": errors.len(),
                        "first_error_len": errors.first().map(|e| e.len()),
                        "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                    })
                });
                if tx.send(AppEvent::CloneExpanded { repos, errors }).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "expand_clone", "send_failed": true}),
                    );
                }
            });
        });
    }

    /// Startup update check (0017 M3): 24h-cached, one network call a
    /// day at most; failures are silent by design. The once-a-day
    /// toast quota (0018 M2) is consumed here, cache-file side —
    /// never on the UI thread.
    pub(crate) fn spawn_update_check(&self) {
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || json!({"job": "update_check"}));
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let event = if let Some(tag) = crate::selfupdate::latest_known()
                    && crate::selfupdate::is_newer(&tag)
                {
                    let toast = crate::selfupdate::take_toast(&tag);
                    rootle_trace::record_with(EventKind::JobFinished, || {
                        json!({
                            "job": "update_check",
                            "outcome": "available",
                            "tag": tag,
                            "toast": toast,
                            "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                        })
                    });
                    AppEvent::UpdateAvailable { tag, toast }
                } else {
                    rootle_trace::record_with(EventKind::JobFinished, || {
                        json!({
                            "job": "update_check",
                            "outcome": "no_notice",
                            "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                        })
                    });
                    return;
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "update_check", "send_failed": true}),
                    );
                }
            });
        });
    }

    /// 0019 M2: the consent-approved install — the same verified
    /// flow as the CLI, through a recorder Ui (no stderr writes
    /// inside the TUI), honoring the config's tag and sha pins.
    pub(crate) fn spawn_declared_install(&self, decl: crate::provider::Declaration) {
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "declared_install",
                    "name": decl.name,
                    "repo": decl.repo,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let r = rootle_manager::ProviderReference {
                    repo: decl.repo.clone(),
                    name: decl.name.clone(),
                    tag: decl.tag.clone(),
                    tarball: None,
                };
                let event = match rootle_manager::Manager::new().and_then(|m| {
                    let (ui, _log) = rootle_manager::progress::ProgressOutput::recorder();
                    m.install_inner(&r, true, &ui, decl.sha.as_deref())
                }) {
                    Ok(_) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "declared_install",
                                "outcome": "ok",
                                "name": decl.name,
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        crate::event::AppEvent::DeclarationInstalled {
                            name: decl.name.clone(),
                        }
                    }
                    Err(e) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "declared_install",
                                "outcome": "err",
                                "name": decl.name,
                                "error": diagnostics::text(&e.to_string()),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        crate::event::AppEvent::DeclarationFailed {
                            name: decl.name.clone(),
                            error: e.to_string(),
                        }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "declared_install", "send_failed": true}),
                    );
                }
            });
        });
    }

    /// Sequential clones on one worker: git is bandwidth-bound anyway,
    /// and per-repo outcomes aggregate into one CloneDone toast.
    pub(crate) fn spawn_clones(&self, repos: Vec<String>, dest: std::path::PathBuf) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let total = repos.len();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "clones",
                    "repos": total,
                    "dest": dest.display().to_string(),
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let mut ok = Vec::new();
                let mut failed = Vec::new();
                for repo in repos {
                    let repo_started = rootle_trace::enabled().then(Instant::now);
                    let outcome = provider
                        .clone_url(&RepoId::from(repo.clone()))
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
                            rootle_trace::record_with(EventKind::JobFinished, || {
                                json!({
                                    "job": "clone",
                                    "outcome": "ok",
                                    "repo": repo,
                                    "duration_us": repo_started.map(|clock| clock.elapsed().as_micros()),
                                })
                            });
                            ok.push(repo);
                        }
                        Err(e) => {
                            rootle_trace::record_with(EventKind::JobFinished, || {
                                json!({
                                    "job": "clone",
                                    "outcome": "err",
                                    "repo": repo,
                                    "error": diagnostics::text(&e),
                                    "duration_us": repo_started.map(|clock| clock.elapsed().as_micros()),
                                })
                            });
                            failed.push((repo, e));
                        }
                    }
                }
                rootle_trace::record_with(EventKind::JobFinished, || {
                    json!({
                        "job": "clones",
                        "outcome": if failed.is_empty() { "ok" } else if ok.is_empty() { "err" } else { "partial" },
                        "ok": ok.len(),
                        "failed": failed.len(),
                        "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                    })
                });
                if tx.send(AppEvent::CloneDone { ok, failed }).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "clones", "send_failed": true}),
                    );
                }
            });
        });
    }

    pub(crate) fn spawn_org_repos(&self, org: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(
                EventKind::JobStarted,
                || json!({"job": "org_repos", "org": org}),
            );
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let event = match provider.org_repos(&org) {
                    Ok(repos) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "org_repos",
                                "outcome": "ok",
                                "org": org,
                                "repos": repos.len(),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::OrgReposLoaded { org, repos }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "org_repos",
                                "outcome": "err",
                                "org": org,
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::OrgReposFailed { org, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "org_repos", "send_failed": true}),
                    );
                }
            });
        });
    }

    pub(crate) fn spawn_tree(&self, owner: String, name: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        // v1.5: the browsed revision, if the switcher set one.
        let ref_ = self.browser.current_ref().map(str::to_string);
        let op = rootle_trace::operation_id();
        rootle_trace::in_operation(op, || {
            rootle_trace::record_with(EventKind::JobStarted, || {
                json!({
                    "job": "tree",
                    "repo": format!("{owner}/{name}"),
                    "ref": ref_,
                })
            });
        });
        let ticket = self.outstanding.track();
        std::thread::spawn(move || {
            let _ticket = ticket;
            rootle_trace::in_operation(op, || {
                let started = rootle_trace::enabled().then(Instant::now);
                let repo_id = RepoId::from(format!("{owner}/{name}"));
                let ref_at = ref_.as_deref().map(GitRef::from);
                let event = match provider.fetch_tree(&repo_id, ref_at.as_ref()) {
                    Ok(tree) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "tree",
                                "outcome": "ok",
                                "repo": repo_id.as_str(),
                                "entries": tree.entries.len(),
                                "truncated": tree.truncated,
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::TreeLoaded {
                            owner,
                            name,
                            entries: tree.entries,
                            truncated: tree.truncated,
                            branch: tree.branch,
                        }
                    }
                    Err(error) => {
                        rootle_trace::record_with(EventKind::JobFinished, || {
                            json!({
                                "job": "tree",
                                "outcome": "err",
                                "repo": repo_id.as_str(),
                                "error": diagnostics::describe_error(&error),
                                "duration_us": started.map(|clock| clock.elapsed().as_micros()),
                            })
                        });
                        AppEvent::TreeFailed { owner, name, error }
                    }
                };
                if tx.send(event).is_err() {
                    rootle_trace::record_with(
                        EventKind::Error,
                        || json!({"job": "tree", "send_failed": true}),
                    );
                }
            });
        });
    }

    /// repo, org marks fan out to ALL the org's repos.
    pub(crate) fn clone_candidates(&self) -> (Vec<String>, Vec<String>) {
        let mut repos: Vec<String> = Vec::new();
        let mut orgs: Vec<String> = Vec::new();
        let marks = self.browser.visual_marks();
        if !marks.is_empty() {
            for mark in marks {
                use crate::components::browser::MarkKey;
                match mark {
                    MarkKey::Organization { organization } => {
                        orgs.push(organization.as_str().to_string())
                    }
                    MarkKey::Repository { repository } | MarkKey::Entry { repository, .. } => {
                        repos.push(repository.as_str().to_string())
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
