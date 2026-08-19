//! Three-pane miller browser over org → repo → dir → file (PLAN.md §1).
//! Yazi semantics: `h` moves focus left into the parent column, where
//! j/k then browse the parent — the child column rebuilds (cascades)
//! from the new selection. `l` drills back in / deeper.
//! Org repos and repo trees arrive asynchronously from the GitHub API;
//! no mock content is ever shown for trees (honest empty until loaded).

use super::pane::{Entry, EntryKind, Pane};
use super::preview::Preview;
use super::vim_input::VimInput;
use crate::action::Action;
use crate::github::TreeNode;
use crate::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::Frame;
use std::collections::{HashMap, HashSet};

/// A repo's full recursive tree, paths relative to the repo root.
pub struct RepoTree {
    pub owner: String,
    pub name: String,
    pub truncated: bool,
    entries: Vec<TreeNode>,
}

impl RepoTree {
    /// Direct children of `path` ("" = root), dirs first, then files,
    /// alphabetical within each group.
    pub fn children(&self, path: &str) -> Vec<Entry> {
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for node in &self.entries {
            let Some(rest) = node.path.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() || rest.contains('/') {
                continue;
            }
            let entry = Entry::new(
                rest,
                if node.is_dir {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                },
            );
            if node.is_dir {
                dirs.push(entry);
            } else {
                files.push(entry);
            }
        }
        let by_name = |a: &Entry, b: &Entry| a.name.cmp(&b.name);
        dirs.sort_by(by_name);
        files.sort_by(by_name);
        dirs.extend(files);
        dirs
    }

    pub fn find(&self, path: &str) -> Option<&TreeNode> {
        self.entries.iter().find(|e| e.path == path)
    }
}

pub struct Browser {
    /// Level stack: levels[i] lists the children of the selection in
    /// levels[i-1]. levels[0] = orgs, levels[1] = repos, 2 = repo root,
    /// 3+ = dirs.
    levels: Vec<Pane>,
    /// Which level owns the keyboard. Default = deepest level.
    focus: usize,
    tree: Option<RepoTree>,
    /// Highlighted blob content by sha (in-memory, session-scoped).
    blobs: HashMap<String, Vec<Line<'static>>>,
    /// Shas with an in-flight fetch (dedupe worker spawns).
    pending_blobs: HashSet<String>,
    /// Set by refresh when the selected file needs a fetch; the app
    /// drains it via `take_blob_request` and routes `LoadBlob`.
    blob_request: Option<(String, String)>,
    pub preview: Preview,
    /// `/` filter input, owned here, active in SEARCH mode.
    pub filter_input: VimInput,
}

impl Default for Browser {
    fn default() -> Self {
        Self::new(&[])
    }
}

impl Browser {
    pub fn new(recent_orgs: &[String]) -> Self {
        let mut names: Vec<String> = recent_orgs.to_vec();
        for d in ["ratatui", "tokio-rs", "helix-editor"] {
            if !names.iter().any(|n| n == d) {
                names.push(d.to_string());
            }
        }
        let orgs = names
            .iter()
            .map(|n| Entry::new(n, EntryKind::Org))
            .collect();
        // Starts folded at the orgs level; the repos level arrives
        // asynchronously via `org_repos_loaded`.
        let mut browser = Browser {
            levels: vec![Pane::new("orgs", orgs)],
            focus: 0,
            tree: None,
            blobs: HashMap::new(),
            pending_blobs: HashSet::new(),
            blob_request: None,
            preview: Preview::new(),
            filter_input: VimInput::transient(),
        };
        browser.sync();
        browser
    }

    /// Selected org at the top level, if any.
    pub fn selected_org(&self) -> Option<String> {
        self.levels[0].selected_entry().map(|e| e.name.clone())
    }

    /// Ensure `org` exists in the orgs level and select it.
    pub fn select_org(&mut self, org: &str) {
        let pos = self.levels[0]
            .entries
            .iter()
            .position(|e| e.name == org)
            .unwrap_or_else(|| {
                self.levels[0]
                    .entries
                    .insert(0, Entry::new(org, EntryKind::Org));
                0
            });
        self.levels[0].select(pos);
        self.focus = 0;
        self.sync();
    }

    /// Org repos arrived from the API: install/replace the repos level.
    /// Ignored if the user has since selected a different org.
    pub fn org_repos_loaded(&mut self, org: &str, repos: Vec<String>) {
        if self.selected_org().as_deref() != Some(org) {
            return;
        }
        let entries = repos
            .iter()
            .map(|r| Entry::new(r, EntryKind::Repo))
            .collect();
        self.levels.truncate(1);
        self.levels.push(Pane::new(org, entries));
        self.focus = 1;
        self.cascade();
        self.sync();
    }

    /// Repo tree arrived: install it and rebuild the dir columns.
    /// Ignored if it doesn't match the currently selected repo.
    pub fn tree_loaded(
        &mut self,
        owner: &str,
        name: &str,
        entries: Vec<TreeNode>,
        truncated: bool,
    ) {
        if self.levels.get(1).map(|p| p.title.as_str()) != Some(owner) {
            return;
        }
        let current_repo = self.levels[1].selected_entry().map(|e| e.name.clone());
        if current_repo.as_deref() != Some(name) {
            return;
        }
        self.tree = Some(RepoTree {
            owner: owner.to_string(),
            name: name.to_string(),
            truncated,
            entries,
        });
        // Rebuild from the repos level: all dir columns were mock/empty.
        self.levels.truncate(2);
        self.focus = 1;
        self.cascade();
        // Complete the interrupted drill: the tree load was requested by
        // acting on this repo, so land the user in its root pane rather
        // than leaving them on the repos pane to press `l` again.
        if self.levels.len() > 2 {
            self.focus = 2;
        }
        self.sync();
    }

    fn current(&mut self) -> &mut Pane {
        &mut self.levels[self.focus]
    }

    /// Recompute focused flags + preview after any state change.
    fn sync(&mut self) {
        for (i, pane) in self.levels.iter_mut().enumerate() {
            pane.focused = i == self.focus;
        }
        self.refresh_preview();
    }

    pub fn context(&self) -> String {
        // Path up to the focused level (level 0 = orgs, not shown).
        self.levels
            .iter()
            .skip(1)
            .take(self.focus)
            .map(|p| p.title.clone())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn selected_kind(&self) -> Option<EntryKind> {
        self.levels[self.focus].selected_entry().map(|e| e.kind)
    }

    /// Repos level title = owner of whatever is being browsed.
    fn current_owner(&self) -> Option<&str> {
        self.levels.get(1).map(|p| p.title.as_str())
    }

    /// Children of an entry: orgs never expand locally (API), repos/dirs
    /// expand from the loaded tree only — no mock trees, ever.
    fn children_of(&self, entry: &Entry) -> Option<Vec<Entry>> {
        let tree = self.tree.as_ref()?;
        match entry.kind {
            EntryKind::Org | EntryKind::File => None,
            EntryKind::Repo => (tree.owner == self.current_owner()? && tree.name == entry.name)
                .then(|| tree.children("")),
            EntryKind::Dir => {
                let base = self.dir_path();
                let child_path = if base.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{base}/{}", entry.name)
                };
                Some(tree.children(&child_path))
            }
        }
    }

    pub fn set_repo(&mut self, owner: &str, name: &str) {
        self.select_org(owner);
        // Reuse the API-loaded repos level if present; otherwise the
        // repos pane waits for the org load (never mock repos for an
        // org we haven't loaded — except the static defaults).
        if self.levels.len() < 2 || self.levels[1].title != owner {
            self.levels.truncate(1);
            self.levels.push(Pane::new(owner, vec![]));
        }
        if let Some(pos) = self.levels[1].entries.iter().position(|e| e.name == name) {
            self.levels[1].select(pos);
        } else {
            self.levels[1]
                .entries
                .push(Entry::new(name, EntryKind::Repo));
            let last = self.levels[1].entries.len() - 1;
            self.levels[1].select(last);
        }
        self.focus = 1;
        self.cascade();
        self.sync();
    }

    pub fn update(&mut self, action: &Action) -> Action {
        match action {
            Action::MoveUp | Action::MoveDown => {
                self.current().update(action);
                // Selection changed in the focused column: every column
                // to its right is stale — rebuild from the new selection.
                self.cascade();
                self.sync();
            }
            Action::DrillIn => {
                let action = self.drill_in();
                self.sync();
                return action;
            }
            Action::DrillOut => {
                if self.focus > 0 {
                    self.focus -= 1;
                }
                self.sync();
            }
            Action::PreviewScrollDown => self.preview.scroll_by(3),
            Action::PreviewScrollUp => self.preview.scroll_by(-3),
            _ => {}
        }
        Action::Noop
    }

    /// Drop levels right of focus and re-derive the immediate child
    /// level from the focused selection. Org entries never cascade —
    /// their repos come from the API (`LoadOrgRepos`).
    fn cascade(&mut self) {
        self.levels.truncate(self.focus + 1);
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            return;
        };
        if let Some(children) = self.children_of(&entry) {
            self.levels.push(Pane::new(entry.name.clone(), children));
        }
    }

    /// Drill into the focused entry. Org entries don't have their repos
    /// locally — the caller must fetch them (`LoadOrgRepos`); repos
    /// likewise fetch their tree (`LoadRepoTree`).
    fn drill_in(&mut self) -> Action {
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            return Action::Noop;
        };
        match entry.kind {
            EntryKind::File => Action::Noop, // OpenSelected handled by app
            EntryKind::Org => Action::LoadOrgRepos(entry.name.clone()),
            EntryKind::Repo => {
                if self.children_of(&entry).is_none() {
                    let owner = self.current_owner().unwrap_or_default().to_string();
                    return Action::LoadRepoTree {
                        owner,
                        name: entry.name,
                    };
                }
                if self.focus == self.levels.len() - 1 {
                    if let Some(children) = self.children_of(&entry) {
                        self.levels.push(Pane::new(entry.name.clone(), children));
                    }
                }
                if self.focus + 1 < self.levels.len() {
                    self.focus += 1;
                }
                Action::Noop
            }
            EntryKind::Dir => {
                if self.focus == self.levels.len() - 1 {
                    if let Some(children) = self.children_of(&entry) {
                        self.levels.push(Pane::new(entry.name.clone(), children));
                    }
                }
                if self.focus + 1 < self.levels.len() {
                    self.focus += 1;
                }
                Action::Noop
            }
        }
    }

    /// Dir path relative to the repo root, from the level titles
    /// (levels: 0=orgs, 1=repos, 2=repo root, 3+=dirs). Also persisted
    /// as `last_path` in the state store.
    pub fn dir_path(&self) -> String {
        self.levels
            .iter()
            .take(self.focus + 1)
            .skip(3)
            .map(|p| p.title.as_str())
            .collect::<Vec<_>>()
            .join("/")
    }

    pub fn apply_filter(&mut self) {
        let filter = self.filter_input.value();
        self.current().set_filter(filter);
        self.cascade();
        self.sync();
    }

    pub fn clear_filter(&mut self) {
        self.filter_input.clear();
        self.current().set_filter(String::new());
        self.cascade();
        self.sync();
    }

    /// The file under the cursor needs its blob fetched, if any.
    /// Taking a request marks the sha in-flight — without this, the
    /// end-of-route drain would re-request on every keystroke and
    /// recurse (stack overflow, caught by the filter-commit test).
    pub fn take_blob_request(&mut self) -> Option<(String, String)> {
        let (sha, name) = self.blob_request.take()?;
        self.pending_blobs
            .insert(sha.clone())
            .then_some((sha, name))
    }

    /// Highlighted blob arrived: store and refresh if still selected.
    pub fn blob_loaded(&mut self, sha: &str, lines: Vec<Line<'static>>) {
        self.blobs.insert(sha.to_string(), lines);
        self.pending_blobs.remove(sha);
        self.refresh_preview();
    }

    pub fn blob_failed(&mut self, _sha: &str, message: &str) {
        // Stay marked pending: no auto-retry while the user keeps
        // moving (avoids hammering a failing endpoint per keystroke).
        self.preview.content = super::preview::PreviewContent::Text(format!("error: {message}"));
    }

    /// The repo coordinates for a blob fetch.
    pub fn repo_coords(&self) -> Option<(String, String)> {
        let owner = self.current_owner()?.to_string();
        let name = self.tree.as_ref()?.name.clone();
        Some((owner, name))
    }

    fn refresh_preview(&mut self) {
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            self.preview.content = Default::default();
            return;
        };
        match entry.kind {
            EntryKind::File => {
                let base = self.dir_path();
                let full = if base.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{base}/{}", entry.name)
                };
                let node = self.tree.as_ref().and_then(|t| t.find(&full)).cloned();
                match node {
                    Some(node) => {
                        if let Some(lines) = self.blobs.get(&node.sha) {
                            let lines = lines.clone();
                            self.preview.set_highlighted(&entry.name, lines);
                        } else {
                            if !self.pending_blobs.contains(&node.sha) {
                                self.blob_request = Some((node.sha.clone(), entry.name.clone()));
                            }
                            self.preview
                                .set_file_meta(&entry.name, node.size, &node.sha);
                        }
                    }
                    None => self.preview.content = Default::default(),
                }
            }
            EntryKind::Dir => {
                let children = self.children_of(&entry).unwrap_or_default();
                self.preview.set_dir(&entry.name, children);
            }
            EntryKind::Repo => {
                let children = self.children_of(&entry).unwrap_or_default();
                self.preview.set_dir(&entry.name, children);
            }
            EntryKind::Org => {
                // Repos load over the API; don't mock them in preview.
                self.preview.title = entry.name.clone();
                self.preview.content = Default::default();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Top level (orgs): fold to a single full-width pane. There is
        // no parent above orgs, so a three-pane split would leave the
        // left column empty (PLAN.md §5).
        if self.focus == 0 {
            self.levels[0].render(frame, area, theme);
            return;
        }

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(40),
                Constraint::Percentage(40),
            ])
            .split(area);

        // Window over the level stack: parent | focused | preview.
        let parent = self.focus - 1;
        self.levels[parent].render(frame, cols[0], theme);
        let focus = self.focus;
        self.levels[focus].render(frame, cols[1], theme);
        self.preview.render(frame, cols[2], theme);
    }
}
