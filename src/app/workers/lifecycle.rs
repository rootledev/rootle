//! Worker spawners — app-lifecycle workers: update check, declared-provider install, clone fan-out, org/tree loads
//! (moved from app/workers.rs, plans/0021 M2 — a pure move).

use super::{App, trace};
use crate::event::AppEvent;

impl App {
    /// Expand org marks to their repos off the UI thread, then the
    /// wizard opens with the combined list. v1.4: expanded repos keep
    /// their listing metadata; direct selections stay bare names.
    pub(crate) fn spawn_expand_clone(&self, repos: Vec<String>, orgs: Vec<String>) {
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

    /// Startup update check (0017 M3): 24h-cached, one network call a
    /// day at most; failures are silent by design. The once-a-day
    /// toast quota (0018 M2) is consumed here, cache-file side —
    /// never on the UI thread.
    pub(crate) fn spawn_update_check(&self) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if let Some(tag) = crate::update::latest_known()
                && crate::update::is_newer(&tag)
            {
                let toast = crate::update::take_toast(&tag);
                let _ = tx.send(AppEvent::UpdateAvailable { tag, toast });
            }
        });
    }

    /// 0019 M2: the consent-approved install — the same verified
    /// flow as the CLI, through a recorder Ui (no stderr writes
    /// inside the TUI), honoring the config's tag and sha pins.
    pub(crate) fn spawn_declared_install(&self, decl: crate::provider::Declaration) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = crate::provider::manager::Ref {
                repo: decl.repo.clone(),
                name: decl.name.clone(),
                tag: decl.tag.clone(),
                tarball: None,
            };
            let event = match crate::provider::manager::Manager::new().and_then(|m| {
                let (ui, _log) = crate::provider::ui::Ui::recorder();
                m.install_inner(&r, true, &ui, decl.sha.as_deref())
            }) {
                Ok(_) => crate::event::AppEvent::DeclarationInstalled {
                    name: decl.name.clone(),
                },
                Err(e) => crate::event::AppEvent::DeclarationFailed {
                    name: decl.name.clone(),
                    error: e.to_string(),
                },
            };
            let _ = tx.send(event);
        });
    }

    /// Sequential clones on one worker: git is bandwidth-bound anyway,
    /// and per-repo outcomes aggregate into one CloneDone toast.
    pub(crate) fn spawn_clones(&self, repos: Vec<String>, dest: std::path::PathBuf) {
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

    pub(crate) fn spawn_org_repos(&self, org: String) {
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

    pub(crate) fn spawn_tree(&self, owner: String, name: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        // v1.5: the browsed revision, if the switcher set one.
        let ref_ = self.browser.current_ref().map(str::to_string);
        std::thread::spawn(move || {
            trace(&format!("tree start {owner}/{name}"));
            let event = match provider.fetch_tree(&format!("{owner}/{name}"), ref_.as_deref()) {
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

    /// repo, org marks fan out to ALL the org's repos.
    pub(crate) fn clone_candidates(&self) -> (Vec<String>, Vec<String>) {
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
