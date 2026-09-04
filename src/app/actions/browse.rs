//! Action dispatch — miller-column navigation: org/repo/tree selection, blobs, filters, visual marks, yank, and the launch repo popup
//! (moved from app/mod.rs, plans/0021 M1 — a pure move, zero behavior
//! change).

use super::super::{App, provider_status, trace};
use crate::action::Action;
use crate::components::pane::EntryKind;
use crate::mode::Mode;

impl App {
    /// This domain's arms: `Some(action)` back when not ours, so the
    /// next domain tries it; arm bodies are moved verbatim.
    pub(crate) fn try_browse(&mut self, action: Action) -> Option<Action> {
        let _consumed: bool = match action {
            Action::SearchSubmitted(_) => {
                self.search_gen += 1;
                self.provider.advise_cancel(); // superseded in-flight work
                self.status = Some(format!("searching {}…", self.modeline.forge));
                if let Some(popup) = &mut self.popup {
                    popup.update(&action);
                }
                if !self.offline {
                    self.spawn_search(self.search_gen);
                }
                true
            }
            Action::SearchResults { .. } | Action::SearchFailed { .. } => {
                // Injected (tests) or misrouted worker outcomes.
                if let Some(popup) = &mut self.popup {
                    popup.update(&action);
                }
                self.status = None;
                true
            }
            Action::RepoSelected { owner, name } => {
                trace(&format!("RepoSelected {owner}/{name}"));
                self.state.record_repo(&owner, &name);
                self.state.save();
                self.browser.set_repo(&owner, &name);
                self.popup = None;
                self.mode = Mode::Browse;
                self.handle_action(Action::LoadRepoTree { owner, name });
                true
            }
            Action::OrgSelected(org) => {
                trace(&format!("OrgSelected {org}"));
                self.state.record_org(&org);
                self.state.save();
                self.browser.select_org(&org);
                self.popup = None;
                self.mode = Mode::Browse;
                self.handle_action(Action::LoadOrgRepos(org));
                true
            }
            Action::LoadOrgRepos(org) => {
                self.status = Some(format!("loading {org}…"));
                if !self.offline {
                    self.spawn_org_repos(org);
                }
                true
            }
            Action::OrgReposLoaded { org, repos } => {
                self.status = None;
                self.browser.org_repos_loaded(&org, repos);
                true
            }
            Action::OrgReposFailed { org, error } => {
                self.status = Some(format!("{org}: {}", provider_status(&error)));
                true
            }
            Action::LoadRepoTree { owner, name } => {
                self.status = Some(format!("loading {owner}/{name} tree…"));
                if !self.offline {
                    self.spawn_tree(owner, name);
                }
                true
            }
            Action::TreeLoaded {
                owner,
                name,
                entries,
                truncated,
                branch,
            } => {
                self.status = None;
                self.browser
                    .tree_loaded(&owner, &name, entries, truncated, branch);
                true
            }
            Action::TreeFailed { owner, name, error } => {
                self.status = Some(format!("{owner}/{name}: {}", provider_status(&error)));
                true
            }
            Action::LoadBlob { sha, name } => {
                if !self.offline {
                    self.spawn_blob(sha, name);
                }
                true
            }
            Action::BlobLoaded { sha, name, bytes } => {
                // Sanitize at the boundary, highlight once, cache in
                // the browser (PLAN.md §9).
                if crate::sanitize::is_binary(&bytes) {
                    self.browser.blob_failed(&sha, "binary file");
                    return None;
                }
                let text = crate::sanitize::sanitize(&bytes);
                let lines = self.highlighter.highlight(&name, &text);
                let lang = self.highlighter.language(&name);
                self.browser.blob_loaded(&sha, &name, &lang, text, lines);
                self.band_apply_or_fetch();
                true
            }
            Action::BlobFailed { sha, error } => {
                self.status = Some(provider_status(&error));
                self.browser.blob_failed(&sha, &error.message);
                true
            }
            Action::HitContextFailed { sha: _, error } => {
                // Auth/throttle surface; anything else stays quiet —
                // the bare path remains and revisit retries (§2).
                use crate::provider::ErrorKind;
                if matches!(error.kind, ErrorKind::Auth | ErrorKind::RateLimited) {
                    self.status = Some(provider_status(&error));
                }
                true
            }
            Action::Visual => {
                self.mode = Mode::Visual;
                self.browser.enter_visual();
                true
            }
            Action::ExitVisual => {
                self.mode = Mode::Browse;
                self.browser.exit_visual();
                true
            }
            Action::ToggleSelect => {
                self.browser.toggle_selected();
                true
            }
            Action::LeaderReload => {
                self.mode = Mode::Browse;
                // Explicit reload is the retry path for failed blobs
                // too (0023): clear the failure cache so the preview
                // re-requests.
                self.browser.retry_failed_blobs();
                if let Some((owner, name)) = self.browser.repo_coords() {
                    // Conditional refetch: cheap when the ref ETag is
                    // still fresh (304), fresh tree when it moved.
                    self.handle_action(Action::LoadRepoTree { owner, name });
                    self.status = Some("reloading tree…".into());
                } else if let Some(org) = self.browser.selected_org() {
                    self.handle_action(Action::LoadOrgRepos(org));
                    self.status = Some("reloading org repos…".into());
                } else {
                    self.status = Some("nothing to reload".into());
                }
                true
            }
            Action::DeleteMarked => {
                self.mode = Mode::Browse;
                let deleted = self.browser.delete_marked_orgs();
                if deleted.is_empty() {
                    self.status = Some("no marked orgs (mark orgs in VISUAL, ␣d)".into());
                } else {
                    // Keep persisted recents in sync.
                    self.state.recent_orgs.retain(|o| !deleted.contains(o));
                    if self
                        .state
                        .last_org
                        .as_deref()
                        .is_some_and(|o| deleted.iter().any(|d| d == o))
                    {
                        self.state.last_org = None;
                    }
                    self.state.save();
                    self.status = Some(format!("deleted {} org(s)", deleted.len()));
                }
                true
            }
            Action::ClearMarks => {
                self.mode = Mode::Browse;
                self.browser.clear_marks();
                self.status = Some("marks cleared".into());
                true
            }
            Action::LeaderYank => {
                // Mock stage (plans/0003 §1): toast the URL that would
                // be yanked; clipboard (OSC 52) wires up later.
                // From the leader layer it drops back to Browse; from
                // the preview submode (␣ p y) the pane stays focused.
                if self.mode == Mode::Leader {
                    self.mode = Mode::Browse;
                }
                // URLs come from the provider — no GitHub grammar
                // outside the GitHub impl (plans/0005).
                let url = if let Some(view) = &self.search_view {
                    view.yank_target().and_then(|t| {
                        self.provider
                            .web_url(&t.repo, &t.path, &t.branch, t.line, t.end, true)
                            .ok()
                    })
                } else if let Some((owner, repo)) = self.browser.repo_coords() {
                    // File under the cursor yanks the FILE (blob URL);
                    // otherwise the current directory (tree URL).
                    let (path, is_file) = match self.browser.selected_file() {
                        Some((file, _sha)) => (file, true),
                        None => (self.browser.dir_path(), false),
                    };
                    let branch = self.browser.branch().unwrap_or("");
                    // File yank anchors to the preview line cursor —
                    // or the visual range as `#L3-L7` (v1.5); dirs/orgs
                    // stay line-less.
                    let (line, end) = if is_file {
                        self.browser.yank_anchor()
                    } else {
                        (None, None)
                    };
                    self.provider
                        .web_url(
                            &format!("{owner}/{repo}"),
                            &path,
                            branch,
                            line,
                            end,
                            is_file,
                        )
                        .ok()
                } else {
                    self.browser
                        .selected_org()
                        .and_then(|org| self.provider.org_url(&org).ok())
                };
                match url {
                    Some(u) => {
                        self.pending_clipboard = Some(u.clone());
                        self.status = Some(format!("yanked {u}"));
                    }
                    None => self.status = Some("nothing to yank".into()),
                }
                true
            }
            Action::EnterSearch => {
                self.browser.filter_input.submode = crate::components::vim_input::SubMode::Insert;
                self.mode = Mode::Search;
                true
            }
            Action::CommitFilter => {
                self.mode = Mode::Browse;
                true
            }
            Action::ClearFilter => {
                // BROWSE Esc precedence (plans/0007 §3): a committed
                // find clears first (:nohlsearch), then the list filter.
                // In SEARCH mode Esc keeps its cancel-the-session role.
                if self.mode == Mode::Browse && self.browser.preview.find_active() {
                    self.browser.preview.clear_find();
                } else {
                    self.browser.clear_filter();
                }
                self.mode = Mode::Browse;
                true
            }
            Action::LeaderFindInFile => {
                // Reachable from the leader layer (Browse underneath)
                // and from the preview submode — FIND returns to
                // whichever raised it.
                let back = self.mode;
                self.find_return = Some(if back == Mode::Preview {
                    Mode::Preview
                } else {
                    Mode::Browse
                });
                self.mode = Mode::Browse; // leader layer down either way
                if self.browser.preview.findable() {
                    self.browser.find_input.clear();
                    self.browser.find_input.submode = crate::components::vim_input::SubMode::Insert;
                    self.browser.preview.begin_find();
                    self.mode = Mode::Find;
                } else {
                    self.mode = self.find_return.take().unwrap_or(Mode::Browse);
                    self.status = Some("find: preview is not a text file".into());
                }
                true
            }
            Action::UpdateFind => {
                let query = self.browser.find_input.value();
                self.browser.preview.update_find(query);
                true
            }
            Action::CommitFind => {
                self.mode = self.find_return.take().unwrap_or(Mode::Browse);
                true
            }
            Action::CancelFind => {
                self.browser.preview.cancel_find();
                self.browser.find_input.clear();
                self.mode = self.find_return.take().unwrap_or(Mode::Browse);
                true
            }
            Action::FindNext => {
                self.browser.preview.find_step(1);
                true
            }
            Action::FindPrev => {
                self.browser.preview.find_step(-1);
                true
            }
            Action::MoveUp
            | Action::MoveDown
            | Action::DrillIn
            | Action::DrillOut
            | Action::PreviewLineUp
            | Action::PreviewLineDown => {
                let follow = self.browser.update(&action);
                self.handle_action(follow);
                true
            }
            Action::OpenSelected => {
                match self.browser.selected_kind() {
                    Some(EntryKind::File) => {
                        // Enter on a file → open in the editor (read-only,
                        // PLAN.md §12). The blocking blob fetch is fine:
                        // the UI is about to suspend anyway.
                        if let (Some((path, sha)), Some((owner, repo))) =
                            (self.browser.selected_file(), self.browser.repo_coords())
                        {
                            match crate::editor::prepare(
                                &self.config,
                                self.provider.as_ref(),
                                &format!("{owner}/{repo}"),
                                &path,
                                &sha,
                            ) {
                                Ok(job) => self.pending_editor = Some(job),
                                Err(message) => self.status = Some(format!("editor: {message}")),
                            }
                        }
                    }
                    Some(EntryKind::Dir | EntryKind::Repo | EntryKind::Org) => {
                        let follow = self.browser.update(&Action::DrillIn);
                        self.handle_action(follow);
                    }
                    None => {}
                }
                true
            }

            _ => return Some(action),
        };
        debug_assert!(_consumed);
        None
    }
}
