//! Action dispatch — the global search view: lifecycle, streamed hit events, and the expanded pane's parity actions
//! (moved from app/mod.rs, plans/0021 M1 — a pure move, zero behavior
//! change).

use super::super::App;
use crate::action::Action;
use crate::components::global_search::{GlobalSearch, SearchKind};
use crate::components::search_popup::SearchPopup;
use crate::event::AppEvent;
use crate::mode::Mode;

impl App {
    /// This domain's arms: `Some(action)` back when not ours, so the
    /// next domain tries it; arm bodies are moved verbatim.
    pub(crate) fn try_search(&mut self, action: Action) -> Option<Action> {
        let _consumed: bool = match action {
            Action::PreviewCopy => {
                // GitHub's copy button: the visual selection, else the
                // cursor line — from whichever file pane is up.
                let target = if let Some(view) = &mut self.search_view {
                    view.expanded_copy_target()
                } else {
                    self.browser.preview.copy_target()
                };
                match target {
                    Some((text, n)) => {
                        self.pending_clipboard = Some(text);
                        self.status =
                            Some(format!("copied {n} line{}", if n == 1 { "" } else { "s" }));
                    }
                    None => self.status = Some("nothing to copy".into()),
                }
                true
            }
            Action::LeaderSearch => {
                // Resume: prefill with the last repo — one Enter
                // re-runs the query back to where the user was.
                let mut popup = SearchPopup::with_prefill(self.state.last_repo.as_deref());
                popup.forge = self.modeline.forge.clone();
                self.popup = Some(popup);
                self.mode = Mode::Browse;
                true
            }
            Action::LeaderFileFind | Action::LeaderGrep => {
                let kind = if action == Action::LeaderFileFind {
                    SearchKind::FileFind
                } else {
                    SearchKind::Grep
                };
                let repo = self
                    .browser
                    .repo_coords()
                    .map(|(owner, name)| format!("{owner}/{name}"));
                let org = self.browser.selected_org();
                let persisted_scope = self
                    .state
                    .search_scope
                    .as_deref()
                    .and_then(crate::components::global_search::Scope::from_stored);
                let mut view = GlobalSearch::new(
                    kind,
                    repo,
                    org,
                    persisted_scope,
                    self.state.search_extension.clone(),
                );
                // plans/0016 M1a: off the default branch, index-backed
                // search (GitHub) can't follow — the title says so.
                if let (Some(r), Some(b)) = (self.browser.current_ref(), self.browser.branch())
                    && r != b
                {
                    view.search_ref_note = Some(format!("search: {b} only"));
                }
                self.search_view = Some(view);
                self.mode = Mode::Browse;
                true
            }
            Action::CloseSearchView => {
                self.search_view = None;
                self.mode = Mode::Browse;
                true
            }
            Action::GlobalSearchSubmitted {
                ref kind,
                ref query,
                ref scope,
                ref extension,
            } => {
                // Persist last-used scope/extension (plans/0002 §6.4).
                if let Some(view) = &self.search_view {
                    self.state.search_scope = Some(view.scope().stored().to_string());
                    self.state.search_extension = Some(view.extension_value());
                    self.state.save();
                }
                self.view_gen += 1;
                self.provider.advise_cancel(); // superseded in-flight work
                self.pending_context_sha = None;
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
                if self.offline {
                    // Tests: inject the mock producer, same Action flow.
                    let hits =
                        crate::components::global_search::mock::hits(*kind, query, extension);
                    let hits = self.finish_hits(hits, *kind, query);
                    if let Some(view) = &mut self.search_view {
                        view.update(&Action::GlobalSearchResults {
                            hits,
                            clipped: false,
                            index: None,
                            client_filtered: 0,
                            unfiltered: vec![],
                        });
                    }
                    self.status = None;
                    // Bare selected hit (beyond the eager preview cap):
                    // ask for its context lazily (plans/0006 §1).
                    let request = self
                        .search_view
                        .as_ref()
                        .and_then(|view| view.context_request());
                    if let Some(action) = request {
                        self.handle_action(action);
                    }
                } else {
                    self.status = Some("searching code…".into());
                    self.spawn_view_search(
                        self.view_gen,
                        *kind,
                        query.clone(),
                        scope.clone(),
                        extension.clone(),
                    );
                }
                true
            }
            Action::GlobalSearchResults { .. }
            | Action::GlobalSearchDelta { .. }
            | Action::GlobalSearchFailed { .. } => {
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
                self.status = None;
                true
            }
            Action::LoadHitContext { hit, query } => {
                // Dedupe repeat selections of an in-flight fetch.
                if self.pending_context_sha.as_deref() == Some(hit.sha.as_str()) {
                    return None;
                }
                if self.offline {
                    return None; // tests inject context via Action directly
                }
                // Cursor-rest debounce (plans/0008 §3): 200ms rearmed
                // per selection change. Holding j through N hits costs
                // one provider call — the resting one — instead of N
                // requests plus N-1 advisory cancels.
                let timer_gen = self
                    .context_debounce_gen
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                let shared = self.context_debounce_gen.clone();
                let tx = self.tx.clone();
                let hit = *hit;
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    if shared.load(std::sync::atomic::Ordering::SeqCst) == timer_gen {
                        let _ = tx.send(AppEvent::HitContextDebounceFired {
                            timer_gen,
                            hit,
                            query,
                        });
                    }
                });
                true
            }
            Action::HitContextDebounceFired {
                timer_gen,
                hit,
                query,
            } => {
                // The timer thread already generation-checked; re-check
                // here — another request may have landed while the
                // event queued.
                if timer_gen
                    != self
                        .context_debounce_gen
                        .load(std::sync::atomic::Ordering::SeqCst)
                {
                    return None;
                }
                if self.pending_context_sha.as_deref() == Some(hit.sha.as_str()) {
                    return None;
                }
                // A different pending fetch is superseded — tell the
                // provider it can stop (v1.1).
                if self.pending_context_sha.is_some() {
                    self.provider.advise_cancel();
                }
                self.pending_context_sha = Some(hit.sha.clone());
                let gen_id = self.view_gen;
                self.spawn_hit_context(gen_id, hit, query);
                true
            }
            Action::HitContextLoaded { .. } | Action::HitContextMissing { .. } => {
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
                true
            }
            // Expanded file pane (plans/0012 M2): fetch the hit's
            // whole blob on a worker; the view shows a loading
            // placeholder until the styled lines land.
            Action::LoadHitFile { hit } => {
                if !self.offline {
                    let gen_id = self.view_gen;
                    self.spawn_hit_file(gen_id, *hit);
                }
                true
            }
            Action::HitFileLoaded { .. } | Action::HitFileFailed { .. } => {
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
                true
            }
            Action::OpenSearchHit(hit) => {
                if !hit.sha.is_empty() {
                    // Real hit: materialize the blob like the browser
                    // does (cache-first; the UI is about to suspend).
                    if hit.repo.contains('/') {
                        match crate::editor::prepare(
                            &self.config,
                            self.provider.as_ref(),
                            &hit.repo,
                            &hit.path,
                            &hit.sha,
                        ) {
                            Ok(job) => self.pending_editor = Some(job),
                            Err(message) => {
                                self.status = Some(format!("editor: {message}"));
                            }
                        }
                    }
                } else {
                    // Mock hits: materialize the stand-in body.
                    let slug = self
                        .search_view
                        .as_ref()
                        .map(|v| v.kind().slug())
                        .unwrap_or("hit");
                    match crate::editor::resolve_program(&self.config) {
                        Some(program) => {
                            // Content-address the mock body like a real blob.
                            use sha2::{Digest, Sha256};
                            // digest 0.11 dropped `LowerHex` on its output —
                            // render the hex by hand.
                            use std::fmt::Write as _;
                            let digest = Sha256::digest(hit.body.as_bytes());
                            let mut sha = String::with_capacity(digest.len() * 2);
                            for byte in digest {
                                let _ = write!(sha, "{byte:02x}");
                            }
                            match crate::editor::materialize(
                                "mock",
                                slug,
                                &sha,
                                &hit.path,
                                hit.body.as_bytes(),
                            ) {
                                Ok(file) => {
                                    let mut args =
                                        crate::editor::build_args(&program, &self.config);
                                    args.push(file.to_string_lossy().into_owned());
                                    self.pending_editor =
                                        Some(crate::editor::EditorJob { program, args });
                                }
                                Err(e) => self.status = Some(format!("editor: {e}")),
                            }
                        }
                        None => {
                            self.status =
                                Some("no editor found — set [editor].program or $EDITOR".into());
                        }
                    }
                }
                true
            }
            _ => return Some(action),
        };
        debug_assert!(_consumed);
        None
    }
}
