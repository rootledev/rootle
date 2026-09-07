//! Worker-event routing: AppEvent → state (moved from app/mod.rs,
//! plans/0021 M1 — a pure move, zero behavior change).
//!
//! 0030: every worker event is traced at receipt, and either lands
//! (`accepted`) or is dropped by an identity guard (`rejected`, with
//! the guard's reason and both sides of the comparison).

use super::diagnostics;
use super::{App, provider, provider_status};
use crate::action::Action;
use crate::components::clone_wizard::CloneWizard;
use crate::event::AppEvent;
use crate::mode::Mode;
use serde_json::json;

impl App {
    pub fn handle_app_event(&mut self, event: AppEvent) {
        rootle_trace::in_operation(rootle_trace::operation_id(), || {
            self.apply_app_event(event);
            diagnostics::record_state(self, "post_event");
        });
    }

    fn apply_app_event(&mut self, event: AppEvent) {
        let name = diagnostics::event_name(&event);
        diagnostics::record_event_received(&event);
        match event {
            AppEvent::SearchResults { gen_id, items } => {
                if !self.search_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_search_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.search_gen,
                        })
                    });
                    return; // stale submission
                }
                self.clear_loading_status(&["searching"]);
                if let Some(popup) = &mut self.popup {
                    popup.update(&Action::SearchResults { items });
                } else {
                    diagnostics::record_event_rejected(
                        name,
                        "search_popup_closed",
                        || json!({"gen":gen_id}),
                    );
                    return;
                }
            }
            AppEvent::SearchFailed { gen_id, error } => {
                if !self.search_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_search_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.search_gen,
                        })
                    });
                    return;
                }
                self.clear_loading_status(&["searching"]);
                if let Some(popup) = &mut self.popup {
                    popup.update(&Action::SearchFailed { error });
                } else {
                    diagnostics::record_event_rejected(
                        name,
                        "search_popup_closed",
                        || json!({"gen":gen_id}),
                    );
                    return;
                }
            }
            AppEvent::OrgReposLoaded { org, repos } => {
                let accepted = self.browser.org_repos_would_accept(&org);
                self.clear_loading_status(&["loading ", "reloading org repos"]);
                self.browser.org_repos_loaded(&org, repos);
                if !accepted {
                    diagnostics::record_event_rejected(name, "org_mismatch", || {
                        json!({
                            "org": org,
                            "selected": self.browser.selected_org(),
                        })
                    });
                    return;
                }
            }
            AppEvent::OrgReposFailed { org, error } => {
                self.status = Some(format!("{org}: {}", provider_status(&error)));
            }
            AppEvent::TreeLoaded {
                owner,
                name: repo,
                entries,
                truncated,
                branch,
            } => {
                let accepted = self.browser.tree_would_accept(&owner, &repo);
                // Rejection identity, allocated only while tracing.
                let requested = rootle_trace::enabled().then(|| format!("{owner}/{repo}"));
                self.handle_action(Action::TreeLoaded {
                    owner,
                    name: repo,
                    entries,
                    truncated,
                    branch,
                });
                if !accepted {
                    diagnostics::record_event_rejected(name, "repo_mismatch", || {
                        json!({
                            "requested": requested,
                            "open": self
                                .browser
                                .repo_coords()
                                .map(|(owner, repo)| format!("{owner}/{repo}")),
                        })
                    });
                    return;
                }
            }
            AppEvent::BlobLoaded { sha, name, bytes } => {
                self.handle_action(Action::BlobLoaded { sha, name, bytes });
            }
            AppEvent::BlobFailed { sha, error } => {
                self.handle_action(Action::BlobFailed { sha, error });
            }
            AppEvent::TreeFailed { owner, name, error } => {
                self.handle_action(Action::TreeFailed { owner, name, error });
            }
            AppEvent::GlobalSearchDelta { gen_id, hits } => {
                if !self.view_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_view_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.view_gen,
                        })
                    });
                    return; // stale batch — a newer submission owns the view
                }
                let Some(view) = &self.search_view else {
                    diagnostics::record_event_rejected(
                        name,
                        "search_view_closed",
                        || json!({"gen": gen_id}),
                    );
                    return;
                };
                let (kind, query) = (view.kind(), view.query.value());
                let hits = hits
                    .into_iter()
                    .map(crate::components::global_search::SearchHit::from_raw)
                    .collect();
                let hits = self.finish_hits(hits, kind, &query);
                if let Some(view) = &mut self.search_view {
                    view.update(&Action::GlobalSearchDelta { hits });
                }
                // Live count while the stream runs.
                if let Some(view) = &self.search_view {
                    self.status = Some(format!(
                        "searching {}… {} hits",
                        self.modeline.forge,
                        view.hit_count()
                    ));
                }
            }
            AppEvent::GlobalSearchResults {
                gen_id,
                hits,
                clipped,
                index,
                client_filtered,
                unfiltered,
            } => {
                if !self.view_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_view_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.view_gen,
                        })
                    });
                    return; // stale submission
                }
                self.clear_loading_status(&["searching code"]);
                let Some(view) = &self.search_view else {
                    diagnostics::record_event_rejected(
                        name,
                        "search_view_closed",
                        || json!({"gen": gen_id}),
                    );
                    return;
                };
                let (kind, query) = (view.kind(), view.query.value());
                let hits = hits
                    .into_iter()
                    .map(crate::components::global_search::SearchHit::from_raw)
                    .collect();
                let hits = self.finish_hits(hits, kind, &query);
                if let Some(view) = &mut self.search_view {
                    view.update(&Action::GlobalSearchResults {
                        hits,
                        clipped,
                        index,
                        client_filtered,
                        unfiltered,
                    });
                }
                // Bare selected hit (beyond the eager preview cap): ask
                // for its context lazily (plans/0006 §1).
                let request = self
                    .search_view
                    .as_ref()
                    .and_then(|view| view.context_request());
                if let Some(action) = request {
                    self.handle_action(action);
                }
            }
            AppEvent::GlobalSearchFailed { gen_id, error } => {
                if !self.view_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_view_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.view_gen,
                        })
                    });
                    return;
                }
                self.clear_loading_status(&["searching code"]);
                if let Some(view) = &mut self.search_view {
                    view.update(&Action::GlobalSearchFailed { error });
                } else {
                    diagnostics::record_event_rejected(
                        name,
                        "search_view_closed",
                        || json!({"gen":gen_id}),
                    );
                    return;
                }
            }
            AppEvent::HitContextDebounceFired {
                timer_gen,
                hit,
                query,
            } => {
                self.handle_action(Action::HitContextDebounceFired {
                    timer_gen,
                    hit,
                    query,
                });
            }
            AppEvent::HitContextMissing { gen_id, sha } => {
                if !self.view_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_view_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.view_gen,
                        })
                    });
                    return; // view moved on
                }
                self.handle_action(Action::HitContextMissing { sha });
            }
            AppEvent::HitContextFailed { gen_id, sha, error } => {
                if !self.view_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_view_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.view_gen,
                        })
                    });
                    return;
                }
                self.handle_action(Action::HitContextFailed { sha, error });
            }
            AppEvent::HitContextLoaded {
                gen_id,
                repo,
                path,
                sha,
                line,
                preview,
                match_count,
                query,
            } => {
                if !self.view_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_view_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.view_gen,
                        })
                    });
                    return; // view moved on
                }
                if self.pending_context_sha.as_deref() == Some(sha.as_str()) {
                    self.pending_context_sha = None;
                }
                let Some(view) = &self.search_view else {
                    diagnostics::record_event_rejected(
                        name,
                        "search_view_closed",
                        || json!({"gen": gen_id}),
                    );
                    return;
                };
                let kind = view.kind();
                let mut hits = vec![crate::components::global_search::SearchHit::plain(
                    &repo,
                    &path,
                    line,
                    preview,
                    match_count,
                    String::new(),
                )];
                hits = self.finish_hits(hits, kind, &query);
                let styled = hits.pop().expect("one hit");
                let action = Action::HitContextLoaded {
                    repo,
                    path,
                    sha,
                    line,
                    preview: styled.preview,
                    match_count,
                };
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
            }
            AppEvent::HitFileLoaded {
                gen_id,
                repo,
                path,
                sha,
                bytes,
            } => {
                if !self.view_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_view_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.view_gen,
                        })
                    });
                    return; // view moved on — drop the stale blob
                }
                // 0019 polish: the expanded pane's band rides the same
                // last-commit memo as the miller preview.
                let band_ctx = {
                    let branch = self
                        .search_view
                        .as_ref()
                        .and_then(|v| v.expanded_branch())
                        .unwrap_or_default();
                    self.last_commits
                        .get(&(repo.clone(), path.clone(), branch))
                        .map(|e| crate::components::preview::BandContext {
                            sha: e.sha.clone(),
                            subject: e.subject.clone(),
                            author: e.author.clone(),
                            date: e.date.clone(),
                        })
                };
                // Sanitize + highlight at the boundary, on the UI
                // thread (PLAN.md §9) — same rule as every blob.
                let action = if crate::sanitize::is_binary(&bytes) {
                    Action::HitFileFailed {
                        error: rootle_provider::ProviderError::other("binary file"),
                        sha,
                    }
                } else {
                    let text = crate::sanitize::sanitize(&bytes);
                    let mut lines = self.highlighter.highlight(&path, &text);
                    let lang = self.highlighter.language(&path);
                    // 0019: the expanded pane wears the same match chips
                    // as the results list — a boundary-aware chip so a
                    // match straddling syntax spans (half in a comment)
                    // still shows.
                    if let Some(view) = &self.search_view
                        && view.is_grep()
                    {
                        let needle = view.query_text().to_lowercase();
                        if !needle.is_empty() {
                            let (bg, fg) =
                                (self.theme.semantic.search_match, self.theme.semantic.crust);
                            for line in &mut lines {
                                crate::components::global_search::chip_line(line, &needle, bg, fg);
                            }
                        }
                    }
                    Action::HitFileLoaded {
                        repo: repo.clone(),
                        path: path.clone(),
                        sha,
                        lang,
                        lines,
                    }
                };
                // Memo miss on the expanded hit: spawn the one-shot
                // fetch (log-capable providers only; ambient).
                if band_ctx.is_none()
                    && !self.offline
                    && let Some(view) = &self.search_view
                    && let Some(branch) = view.expanded_branch()
                {
                    self.spawn_last_commit(repo.clone(), path.clone(), Some(branch.clone()));
                }
                if let Some(view) = &mut self.search_view {
                    view.expanded_set_band(&path, band_ctx);
                    view.update(&action);
                }
            }
            AppEvent::HitFileFailed { gen_id, sha, error } => {
                if !self.view_gen.is_current(gen_id) {
                    diagnostics::record_event_rejected(name, "stale_view_generation", || {
                        json!({
                            "gen": gen_id,
                            "current": self.view_gen,
                        })
                    });
                    return;
                }
                // Auth/throttle surface a status line; other kinds
                // stay quiet — the pane itself shows the error
                // (same rule as the lazy context, plans/0008 §2).
                use rootle_provider::ErrorKind;
                if matches!(error.kind, ErrorKind::Auth | ErrorKind::RateLimited) {
                    self.status = Some(provider_status(&error));
                }
                if let Some(view) = &mut self.search_view {
                    view.update(&Action::HitFileFailed { sha, error });
                }
            }
            AppEvent::CloneExpanded { repos, errors } => {
                if repos.is_empty() {
                    self.status = Some(if errors.is_empty() {
                        "nothing to clone".into()
                    } else {
                        format!("no repos: {}", errors.join("; "))
                    });
                } else {
                    if !errors.is_empty() {
                        self.status = Some(format!("some orgs failed: {}", errors.join("; ")));
                    }
                    let cwd = std::env::current_dir().unwrap_or_default();
                    self.wizard = Some(CloneWizard::new(repos, cwd));
                }
            }
            AppEvent::CloneDone { ok, failed } => {
                let mut status = format!(
                    "cloned {} repo{}",
                    ok.len(),
                    if ok.len() == 1 { "" } else { "s" }
                );
                if !failed.is_empty() {
                    status.push_str(&format!(
                        ", {} failed ({} …)",
                        failed.len(),
                        failed[0].1.chars().take(40).collect::<String>()
                    ));
                }
                self.status = Some(status);
            }
            // v1.5 revision lenses (plans/0016 M1).
            AppEvent::RefsLoaded { repo: _, refs } => {
                if let Some(popup) = &mut self.refs_popup {
                    popup.set_refs(refs);
                } else {
                    // No switcher open: the fetch outlived its popup.
                    diagnostics::record_event_rejected(
                        name,
                        "refs_popup_closed",
                        || json!({"branches": refs.branches.len(), "tags": refs.tags.len()}),
                    );
                    return;
                }
            }
            AppEvent::RefsFailed { repo: _, error } => {
                self.status = Some(provider_status(&error));
            }
            AppEvent::LogLoaded {
                path,
                entries,
                truncated,
            } => {
                let accepted = self.browser.history_path() == Some(path.as_str());
                if accepted {
                    self.browser.history_loaded(entries, truncated);
                } else {
                    diagnostics::record_event_rejected(name, "history_path_mismatch", || {
                        json!({
                            "path": path,
                            "lens": self.browser.history_path(),
                        })
                    });
                    return;
                }
            }
            AppEvent::LogFailed { path: _, error } => {
                self.status = Some(provider_status(&error));
            }
            AppEvent::CommitLoaded { request, detail } => {
                // CommitView verifies the request before preparing display
                // strings; raw repo/path/content identities are never sanitized.
                let accepted = self
                    .browser
                    .commit_ref()
                    .is_some_and(|view| view.request() == &request);
                match detail {
                    Ok(detail) => self.browser.commit_loaded(&request, detail),
                    Err(error) => self
                        .browser
                        .commit_failed(&request, provider_status(&error)),
                }
                if !accepted {
                    // The open viewer is a different request — the
                    // landing is dropped by its identity check.
                    diagnostics::record_event_rejected(name, "stale_commit_request", || {
                        json!({
                            "gen": request.generation,
                            "open": self.browser.commit_ref().map(|view| {
                                view.request().generation
                            }),
                        })
                    });
                    return;
                }
            }
            AppEvent::BlameLoaded { path, ranges } => {
                // Which surface takes the landing; the browser applies
                // the ranges only when its lens awaits this path (the
                // identity check inside blame_store).
                let search_takes = self
                    .search_view
                    .as_ref()
                    .is_some_and(|view| view.blame_loading_for(&path));
                let browser_applies = !search_takes && self.browser.blame_awaiting(&path);
                let path_len = path.len();
                if search_takes {
                    if let Some(view) = &mut self.search_view {
                        view.blame_store(path, ranges);
                    }
                } else {
                    self.browser.blame_store(path, ranges);
                }
                // The "blame…" transient has had its say — clear it,
                // but never erase a NEWER status (scoped compare).
                if self.status.as_deref() == Some("blame…") {
                    self.status = None;
                }
                if !search_takes && !browser_applies {
                    diagnostics::record_event_rejected(
                        name,
                        "blame_not_awaiting",
                        || json!({"path_len": path_len}),
                    );
                    return;
                }
            }
            AppEvent::LastCommitLoaded { repo, path, entry } => {
                if let Some(entry) = entry {
                    // Compact at the boundary: 7-char sha, date-only
                    // — the band is a header, not a ledger.
                    let ctx = crate::components::preview::BandContext {
                        sha: entry.sha.chars().take(7).collect(),
                        subject: entry.subject.clone(),
                        author: entry.author.clone(),
                        date: entry
                            .date
                            .split('T')
                            .next()
                            .unwrap_or(&entry.date)
                            .to_string(),
                    };
                    let ref_ = self
                        .browser
                        .current_ref()
                        .map(str::to_string)
                        .unwrap_or_default();
                    self.last_commits
                        .insert((repo.clone(), path.clone(), ref_), entry);
                    // Dress whichever surface is showing this file now:
                    // the miller preview (not at-commit) and/or the
                    // search pane's expanded hit.
                    if !self.browser.at_commit_view()
                        && self.browser.selected_file().is_some_and(|(p, _)| p == path)
                    {
                        self.browser
                            .preview
                            .set_band(Some(path.clone()), Some(ctx.clone()));
                    }
                    if let Some(view) = &mut self.search_view {
                        view.expanded_set_band(&path, Some(ctx));
                    }
                }
            }
            AppEvent::BlameFailed { path, error } => {
                if let Some(view) = &mut self.search_view
                    && view.blame_loading_for(&path)
                {
                    view.blame_clear();
                }
                self.status = Some(provider_status(&error));
            }
            AppEvent::BlobAtLoaded {
                path,
                ref_,
                sha,
                bytes,
                subject,
                author,
                date,
            } => {
                // Open-at-commit: style like every blob, but show it
                // directly — the tree cursor still names the
                // present-day blob, so refresh_preview would revert it.
                if crate::sanitize::is_binary(&bytes) {
                    self.status = Some("binary file at that commit".into());
                    diagnostics::record_event_rejected(
                        name,
                        "binary_blob",
                        || json!({"sha": sha, "bytes": bytes.len()}),
                    );
                    return;
                }
                let text = crate::sanitize::sanitize(&bytes);
                let short: String = ref_.chars().take(7).collect();
                // The title carries the commit marker; highlighting and
                // language detection read the real path — "main.rs @
                // 42ec959" has no known extension (the demo caught the
                // unhighlighted frame).
                let name = format!("{path} @ {short}");
                let lines = self.highlighter.highlight(&path, &text);
                let lang = self.highlighter.language(&path);
                let band = crate::components::preview::BandContext {
                    sha: short,
                    subject,
                    author,
                    date,
                };
                self.browser
                    .show_at_commit(&sha, &name, &lang, text, lines, Some(band));
                // The lens' work is done — the commit's content is up.
                self.browser.close_history();
                self.history_return = Some(Mode::Preview);
                self.mode = Mode::Preview;
                self.status = None;
            }
            AppEvent::BlobAtFailed { path: _, error } => {
                self.status = Some(provider_status(&error));
            }
            AppEvent::UpdateAvailable { tag, toast } => {
                self.update_tag = Some(tag.clone());
                // 0018 M2: the toast nags once a day and never steals
                // the status line from real work — the chip is the
                // persistent channel.
                if toast && self.status.is_none() {
                    self.status = Some(format!("rootle {tag} is out — run `rootle update`"));
                }
            }
            // 0019 M2: the consent install landed — hot-swap the
            // provider, drop the popup, say so.
            AppEvent::DeclarationInstalled { name } => {
                self.consent = None;
                self.degraded = None;
                // 0030: the hot swap is a provider lifecycle event;
                // the operation id correlates transport records from
                // the spawned child.
                let op = rootle_trace::operation_id();
                let swap = rootle_trace::in_operation(op, || {
                    rootle_trace::record_with(rootle_trace::EventKind::ProviderLifecycle, || {
                        json!({
                            "stage": "hot_swap",
                            "name": name,
                        })
                    });
                    let swapped = provider::spawn_installed(&self.config, &name);
                    rootle_trace::record_with(rootle_trace::EventKind::ProviderLifecycle, || {
                        json!({
                            "stage": "hot_swap",
                            "name": name,
                            "outcome": if swapped.is_ok() { "ok" } else { "err" },
                            "error_len": swapped.as_ref().err().map(|e| e.len()),
                        })
                    });
                    swapped
                });
                match swap {
                    Ok(p) => {
                        self.provider = p;
                        self.status = Some(format!("{name} ready"));
                    }
                    Err(e) => {
                        let note = format!("{name} unavailable: {e} — browsing github");
                        self.degraded = Some(note.clone());
                        self.status = Some(note);
                    }
                }
            }
            AppEvent::DeclarationFailed { name, error } => {
                // Honest degraded mode — the popup shows the error
                // until dismissed, then the notice goes sticky.
                let note = format!("{name} unavailable: {error} — browsing github");
                self.degraded = Some(note.clone());
                if let Some(popup) = &mut self.consent {
                    popup.set_state(crate::action::DeclarationState::Failed(error));
                } else {
                    self.status = Some(note);
                }
            }
        }
        diagnostics::record_event_accepted(name);
    }
}
