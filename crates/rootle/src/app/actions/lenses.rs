//! Action dispatch — revision lenses (plans/0016): refs, history, blame, and the preview submode's enter/exit
//! (moved from app/mod.rs, plans/0021 M1 — a pure move, zero behavior
//! change).

use super::super::App;
use super::super::provider_status;
use crate::action::Action;
use crate::components::Component;
use crate::components::refs_popup::RefsPopup;
use crate::mode::Mode;
use rootle_provider::{GitRef, RepoId};

impl App {
    /// This domain's arms: `Some(action)` back when not ours, so the
    /// next domain tries it; arm bodies are moved verbatim.
    pub(crate) fn try_lenses(&mut self, action: Action) -> Option<Action> {
        let _consumed: bool = match action {
            Action::LeaderRefs => {
                self.mode = Mode::Browse;
                if !self.provider.capabilities().refs {
                    self.status = Some("provider has no revision listing".into());
                } else if let Some((owner, name)) = self.browser.repo_coords() {
                    let current = self
                        .browser
                        .current_ref()
                        .or_else(|| self.browser.branch())
                        .unwrap_or("main")
                        .to_string();
                    self.refs_baseline = self.browser.current_ref().map(str::to_string);
                    self.refs_popup = Some(RefsPopup::new(&current));
                    self.spawn_refs(format!("{owner}/{name}"));
                } else {
                    self.status = Some("open a repo to switch revisions".into());
                }
                true
            }
            Action::RefsPreview(name) => {
                // Live preview: the crumb follows the cursor (no fetch
                // until commit).
                self.browser.set_current_ref(Some(name));
                true
            }
            Action::RefsCommit(name) => {
                self.refs_popup = None;
                self.refs_baseline = Some(name.clone());
                self.browser.set_current_ref(Some(name.clone()));
                self.status = Some(format!("switched to {name}"));
                // The ref is read at spawn — refetch the tree now.
                if let Some((owner, name)) = self.browser.repo_coords() {
                    self.handle_action(Action::LoadRepoTree { owner, name });
                }
                true
            }
            Action::LeaderPreview => {
                self.mode = Mode::Browse;
                if self.browser.repo_coords().is_none() {
                    self.status = Some("open a repo first".into());
                } else {
                    self.mode = Mode::Preview;
                }
                true
            }
            Action::ExitPreview => {
                // The commit-view ladder: Esc restores the present-day
                // blob first, exits the submode second.
                if self.browser.preview.clear_visual() {
                    return None;
                } else if self.browser.at_commit_view() {
                    self.browser.commit_view_close();
                } else {
                    self.mode = Mode::Browse;
                }
                true
            }
            Action::BlameToggle => {
                // 0019 parity: the search view's expanded pane runs the
                // same lens at the hit's default branch.
                if let Some(view) = &mut self.search_view {
                    if view.blame_active() {
                        view.blame_clear();
                    } else if !self.provider.capabilities().blame {
                        self.status = Some("provider has no blame".into());
                    } else if !view.blame_toggle_on() {
                        self.status = Some("blame: preview is not a text file".into());
                    } else if let Some((repo, path, branch)) = view.blame_needed() {
                        view.blame_mark_loading(path.clone());
                        let ref_ = if branch.is_empty() {
                            None
                        } else {
                            Some(branch)
                        };
                        self.spawn_blame(repo, path, ref_);
                        self.status = Some("blame…".into());
                    }
                } else if self.browser.preview.blaming() {
                    self.browser.clear_blame();
                } else if !self.provider.capabilities().blame {
                    // Honest absence (Bitbucket has no blame API).
                    self.status = Some("provider has no blame".into());
                } else if !self.browser.blame_toggle_on() {
                    self.status = Some("blame: preview is not a text file".into());
                } else if let Some(path) = self.browser.blame_needed_for()
                    && let Some((owner, name)) = self.browser.repo_coords()
                {
                    self.browser.blame_mark_loading(path.clone());
                    let ref_ = self.browser.current_ref().map(str::to_string);
                    self.spawn_blame(format!("{owner}/{name}"), path, ref_);
                    self.status = Some("blame…".into());
                }
                true
            }
            Action::PreviewEnter => {
                // Blaming: Enter opens the line's commit in the history
                // lens; otherwise Enter is the editor handoff.
                match self.browser.blame_line_sha() {
                    Some(sha) if self.provider.capabilities().log => {
                        if self.browser.open_history(Some(sha)) {
                            self.open_history_fetch();
                            self.history_return = Some(Mode::Preview);
                            self.mode = Mode::History;
                        }
                    }
                    _ => self.handle_action(Action::OpenSelected),
                }
                true
            }
            Action::LeaderHistory => {
                let back = self.mode;
                self.mode = Mode::Browse;
                if !self.provider.capabilities().log {
                    self.status = Some("provider has no commit log".into());
                } else if self.browser.open_history(None) {
                    self.history_return = Some(if back == Mode::Preview {
                        Mode::Preview
                    } else {
                        Mode::Browse
                    });
                    self.open_history_fetch();
                    self.mode = Mode::History;
                } else {
                    self.status = Some("preview a file for its history".into());
                }
                true
            }
            Action::HistoryFilterBegin => {
                self.browser.history_begin_filter();
                true
            }
            Action::HistoryYank => {
                // The permalink that never rots: the URL carries the
                // commit sha as its ref.
                let target = self.browser.repo_coords().zip(self.browser.history_pick());
                if let Some(((owner, name), (path, sha))) = target {
                    match self.provider.web_url(
                        &RepoId::from(format!("{owner}/{name}")),
                        &path,
                        Some(&GitRef::from(sha.as_str())),
                        None,
                        None,
                        true,
                    ) {
                        Ok(u) => {
                            self.pending_clipboard = Some(u.clone());
                            self.status = Some(format!("yanked {u}"));
                        }
                        Err(e) => self.status = Some(provider_status(&e)),
                    }
                }
                true
            }
            Action::HistoryUp => {
                self.browser.history_move(-1);
                true
            }
            Action::HistoryDown => {
                self.browser.history_move(1);
                true
            }
            Action::HistoryOpen => {
                // Open the file at the picked commit — bytes land via
                // BlobAtLoaded; the restore point is noted there.
                let target = self.browser.repo_coords().zip(self.browser.history_pick());
                let entry = self.browser.history_pick_entry();
                if let (Some(((owner, name), (path, sha))), Some(entry)) = (target, entry) {
                    self.spawn_blob_at(
                        format!("{owner}/{name}"),
                        path,
                        sha,
                        entry.subject,
                        entry.author,
                        entry.date,
                    );
                    self.status = Some("opening at commit".into());
                }
                true
            }
            // -- commit viewer (plans/0028) --------------------------------
            Action::CommitDive => {
                if !self.provider.capabilities().commit {
                    self.status = Some("provider has no commit detail".into());
                } else if let Some(((owner, name), (_path, sha))) =
                    self.browser.repo_coords().zip(self.browser.history_pick())
                {
                    let request = crate::request::CommitRequest {
                        repository: format!("{owner}/{name}").into(),
                        revision: sha.into(),
                        generation: self.commit_generation.tick(),
                    };
                    self.browser.open_commit(request.clone());
                    self.spawn_commit(request);
                    self.mode = Mode::Commit;
                }
                true
            }
            command @ (Action::CommitUp
            | Action::CommitDown
            | Action::CommitFirst
            | Action::CommitLast
            | Action::CommitOpen
            | Action::CommitStepNext
            | Action::CommitStepPrev
            | Action::CommitFocus
            | Action::CommitLeft
            | Action::CommitRight
            | Action::CommitFilterBegin
            | Action::CommitFilterKey(_)) => {
                if let Some(view) = self.browser.commit() {
                    view.update(&command);
                }
                true
            }
            Action::CommitYank => {
                let url = self
                    .browser
                    .commit_ref()
                    .and_then(|view| view.web_url())
                    .map(str::to_string);
                if let Some(url) = url {
                    self.status = Some(format!("yanked {url}"));
                    self.pending_clipboard = Some(url);
                } else {
                    self.status = Some("provider has no commit permalink".into());
                }
                true
            }
            Action::CommitClose => {
                // The unwind ladder: delta → detail → filter → history.
                let close = self
                    .browser
                    .commit()
                    .map(|view| view.escape())
                    .unwrap_or(true);
                if close {
                    self.browser.close_commit();
                    self.mode = if self.browser.history_is_open() {
                        Mode::History
                    } else {
                        self.history_return.take().unwrap_or(Mode::Browse)
                    };
                }
                true
            }
            Action::HistoryFilterKey(key) => {
                self.browser.history_filter_key(key);
                true
            }
            Action::PreviewVisualToggle => {
                self.browser.preview.toggle_visual();
                true
            }
            Action::HistoryClose => {
                // The wizard ladder: a committed filter clears first,
                // the next Esc closes.
                if self.browser.history_esc() {
                    self.mode = self.history_return.take().unwrap_or(Mode::Browse);
                }
                true
            }
            _ => return Some(action),
        };
        debug_assert!(_consumed);
        None
    }
}
