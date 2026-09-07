//! Navigation for Browser.

use super::{Action, Browser, Entry, EntryKind, Pane};

impl Browser {
    pub(super) fn current(&mut self) -> &mut Pane {
        &mut self.levels[self.focus]
    }

    /// Recompute focused flags + preview after any state change.
    pub(super) fn sync(&mut self) {
        for column in 0..self.levels.len() {
            let pane_marks = self.marked_names(column);
            let pane = &mut self.levels[column];
            pane.focused = column == self.focus;
            pane.checkboxes = if self.visual || !pane_marks.is_empty() {
                Some(pane_marks)
            } else {
                None
            };
            pane.marks_only = !self.visual;
        }
        self.refresh_preview();
    }

    /// Repos level title = owner of whatever is being browsed.
    pub(super) fn current_owner(&self) -> Option<&str> {
        self.levels.get(1).map(|p| p.title.as_str())
    }

    /// Children of an entry: orgs never expand locally (API), repos/dirs
    /// expand from the loaded tree only — no mock trees, ever.
    pub(super) fn children_of(&self, entry: &Entry) -> Option<Vec<Entry>> {
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
            Action::PreviewLineDown => self.preview.move_cursor(1),
            Action::PreviewLineUp => self.preview.move_cursor(-1),
            _ => {}
        }
        Action::Noop
    }

    /// Drop levels right of focus and re-derive the immediate child
    /// level from the focused selection. Org entries never cascade —
    /// their repos come from the API (`LoadOrgRepos`).
    pub(super) fn cascade(&mut self) {
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
    pub(super) fn drill_in(&mut self) -> Action {
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
                if self.focus == self.levels.len() - 1
                    && let Some(children) = self.children_of(&entry)
                {
                    self.levels.push(Pane::new(entry.name.clone(), children));
                }
                if self.focus + 1 < self.levels.len() {
                    self.focus += 1;
                }
                Action::Noop
            }
            EntryKind::Dir => {
                if self.focus == self.levels.len() - 1
                    && let Some(children) = self.children_of(&entry)
                {
                    self.levels.push(Pane::new(entry.name.clone(), children));
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

    pub fn selected_kind(&self) -> Option<EntryKind> {
        self.levels[self.focus].selected_entry().map(|e| e.kind)
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

    /// The file under the cursor, as (full repo-relative path, blob sha).
    pub fn selected_file(&self) -> Option<(String, String)> {
        let entry = self.levels[self.focus].selected_entry()?;
        if entry.kind != EntryKind::File {
            return None;
        }
        let base = self.dir_path();
        let full = if base.is_empty() {
            entry.name.clone()
        } else {
            format!("{base}/{}", entry.name)
        };
        let sha = self.tree.as_ref()?.find(&full)?.sha.clone();
        Some((full, sha))
    }

    /// The repo coordinates for a blob fetch.
    pub fn repo_coords(&self) -> Option<(String, String)> {
        let owner = self.current_owner()?.to_string();
        let name = self.tree.as_ref()?.name.clone();
        Some((owner, name))
    }
}
