//! Worker-event routing: AppEvent → state (moved from app/mod.rs,
//! plans/0021 M1 — a pure move, zero behavior change).

use super::provider_status;
use super::{App, provider};
use crate::action::Action;
use crate::components::clone_wizard::CloneWizard;
use crate::event::AppEvent;
use crate::mode::Mode;

impl App {
    pub fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::SearchResults { gen_id, items } => {
                if gen_id != self.search_gen {
                    return; // stale submission
                }
                self.clear_loading_status(&["searching"]);
                if let Some(popup) = &mut self.popup {
                    popup.update(&Action::SearchResults { items });
                }
            }
            AppEvent::SearchFailed { gen_id, error } => {
                if gen_id != self.search_gen {
                    return;
                }
                self.clear_loading_status(&["searching"]);
                if let Some(popup) = &mut self.popup {
                    popup.update(&Action::SearchFailed { error });
                }
            }
            AppEvent::OrgReposLoaded { org, repos } => {
                self.clear_loading_status(&["loading ", "reloading org repos"]);
                self.browser.org_repos_loaded(&org, repos);
            }
            AppEvent::OrgReposFailed { org, error } => {
                self.status = Some(format!("{org}: {}", provider_status(&error)));
            }
            AppEvent::TreeLoaded {
                owner,
                name,
                entries,
                truncated,
                branch,
            } => {
                self.handle_action(Action::TreeLoaded {
                    owner,
                    name,
                    entries,
                    truncated,
                    branch,
                });
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
                if gen_id != self.view_gen {
                    return; // stale batch — a newer submission owns the view
                }
                let Some(view) = &self.search_view else {
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
                if gen_id != self.view_gen {
                    return; // stale submission
                }
                self.clear_loading_status(&["searching code"]);
                let Some(view) = &self.search_view else {
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
                if gen_id != self.view_gen {
                    return;
                }
                self.clear_loading_status(&["searching code"]);
                if let Some(view) = &mut self.search_view {
                    view.update(&Action::GlobalSearchFailed { error });
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
                if gen_id != self.view_gen {
                    return; // view moved on
                }
                self.handle_action(Action::HitContextMissing { sha });
            }
            AppEvent::HitContextFailed { gen_id, sha, error } => {
                if gen_id != self.view_gen {
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
                if gen_id != self.view_gen {
                    return; // view moved on
                }
                if self.pending_context_sha.as_deref() == Some(sha.as_str()) {
                    self.pending_context_sha = None;
                }
                let Some(view) = &self.search_view else {
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
                if gen_id != self.view_gen {
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
                        error: crate::provider::ProviderError::other("binary file"),
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
                if gen_id != self.view_gen {
                    return;
                }
                // Auth/throttle surface a status line; other kinds
                // stay quiet — the pane itself shows the error
                // (same rule as the lazy context, plans/0008 §2).
                use crate::provider::ErrorKind;
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
                if self.browser.history_path() == Some(path.as_str()) {
                    self.browser.history_loaded(entries, truncated);
                }
            }
            AppEvent::LogFailed { path: _, error } => {
                self.status = Some(provider_status(&error));
            }
            AppEvent::BlameLoaded { path, ranges } => {
                if let Some(view) = &mut self.search_view
                    && view.blame_loading_for(&path)
                {
                    view.blame_store(path, ranges);
                } else {
                    self.browser.blame_store(path, ranges);
                }
                // The "blame…" transient has had its say — clear it,
                // but never erase a NEWER status (scoped compare).
                if self.status.as_deref() == Some("blame…") {
                    self.status = None;
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
                match provider::spawn_installed(&self.config, &name) {
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
    }
}
