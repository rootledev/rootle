//! Accepted requests own readiness. Cached display data is a separate observation.

use super::{Browser, Entry, EntryKind, Pane, RepoTree};
use crate::request::{LoadPhase, OwnerListRequest, TreeRequest};
use rootle_provider::{GitRef, ProviderError, RepoId, RepoInfo, TreeNode};
use serde_json::{Value, json};

impl Browser {
    pub fn owner_request(&self) -> Option<&OwnerListRequest> {
        self.owner_load.request.as_ref()
    }

    pub fn tree_request(&self) -> Option<&TreeRequest> {
        self.tree_load.request.as_ref()
    }

    pub fn begin_owner_load(&mut self, owner: &str) -> OwnerListRequest {
        let request = OwnerListRequest {
            owner: owner.into(),
            generation: self.owner_generation.tick(),
        };
        self.owner_load.start(request.clone());
        self.owner_entry_count = None;
        self.owner_load_owns_view = true;
        request
    }

    pub fn begin_tree_load(&mut self, owner: &str, name: &str) -> TreeRequest {
        let request = TreeRequest {
            repository: format!("{owner}/{name}").into(),
            revision: self.current_ref().map(GitRef::from),
            generation: self.tree_generation.tick(),
        };
        self.tree_load.start(request.clone());
        self.owner_load_owns_view = false;
        request
    }

    fn selected_repository(&self) -> Option<RepoId> {
        self.repo_coords()
            .map(|(owner, name)| format!("{owner}/{name}").into())
    }

    pub(super) fn invalidate_changed_requests(&mut self) {
        if self
            .owner_request()
            .is_some_and(|request| Some(request.owner.as_str()) != self.selected_owner())
        {
            self.owner_load = Default::default();
            self.owner_entry_count = None;
        }
        if self.tree_request().is_some_and(|request| {
            Some(&request.repository) != self.selected_repository().as_ref()
                || request.revision.as_ref().map(GitRef::as_str) != self.current_ref()
        }) {
            self.tree_load = Default::default();
        }
    }

    pub(crate) fn org_repos_would_accept(&self, request: &OwnerListRequest) -> bool {
        self.owner_load.accepts(request) && self.selected_owner() == Some(request.owner.as_str())
    }

    pub(crate) fn tree_would_accept(&self, request: &TreeRequest) -> bool {
        self.tree_load.accepts(request)
            && self.selected_repository().as_ref() == Some(&request.repository)
            && self.current_ref() == request.revision.as_ref().map(GitRef::as_str)
    }

    pub fn org_repos_loaded(&mut self, request: &OwnerListRequest, repos: Vec<RepoInfo>) {
        if !self.org_repos_would_accept(request) {
            return;
        }
        self.owner_load.finish(None);
        self.owner_entry_count = Some(repos.len());
        // An owner listing may finish after a direct tree load. It must not replace it.
        if !self.owner_load_owns_view || self.focus != 0 {
            return;
        }
        let entries = repos
            .iter()
            .map(|repo| Entry::new(&repo.name, EntryKind::Repo))
            .collect();
        self.levels.truncate(1);
        self.levels.push(Pane::new(request.owner.as_str(), entries));
        self.focus = 1;
        self.cascade();
        self.sync();
    }

    pub fn org_repos_failed(&mut self, request: &OwnerListRequest, error: &ProviderError) {
        if self.org_repos_would_accept(request) {
            self.owner_load.finish(Some(error));
        }
    }

    pub fn tree_loaded(
        &mut self,
        request: &TreeRequest,
        entries: Vec<TreeNode>,
        truncated: bool,
        branch: String,
    ) {
        if !self.tree_would_accept(request) {
            return;
        }
        let Some((owner, name)) = self.repo_coords() else {
            return;
        };
        self.tree_load.finish(None);
        self.tree_source = Some(request.clone());
        self.tree = Some(RepoTree::new(owner, name, truncated, branch, entries));
        // Keep a user's move back to the owner column; arrival is not a new drill command.
        if self.focus == 0 {
            return;
        }
        self.levels.truncate(2);
        self.focus = 1;
        self.cascade();
        if self.levels.len() > 2 {
            self.focus = 2;
        }
        self.sync();
    }

    pub fn tree_failed(&mut self, request: &TreeRequest, error: &ProviderError) {
        if self.tree_would_accept(request) {
            self.tree_load.finish(Some(error));
        }
    }

    pub(crate) fn reset_provider(&mut self, owners: &[String]) {
        let tree_generation = self.tree_generation.tick();
        let owner_generation = self.owner_generation.tick();
        *self = Self::new(owners);
        self.tree_generation = tree_generation;
        self.owner_generation = owner_generation;
    }

    pub(crate) fn load_failed(&self) -> bool {
        if self.focus == 0 {
            self.owner_load_owns_view && self.owner_load.phase == LoadPhase::Failed
        } else {
            self.tree_load.phase == LoadPhase::Failed
        }
    }

    pub fn load_status(&self) -> Option<String> {
        if self.focus == 0 && !self.owner_load_owns_view {
            return None;
        }
        let (phase, error) = if self.focus == 0 {
            (self.owner_load.phase, self.owner_load.error.as_ref())
        } else {
            (self.tree_load.phase, self.tree_load.error.as_ref())
        };
        if let Some(error) = error {
            return Some(format!("{}: {}", error.kind_name(), error.message));
        }
        (phase == LoadPhase::Loading).then(|| {
            if self.focus == 0 {
                "loading owner repositories…"
            } else {
                "loading repository tree…"
            }
            .into()
        })
    }

    pub fn observation(&self) -> Value {
        let current = (self.tree_load.phase == LoadPhase::Ready)
            .then_some(self.tree.as_ref())
            .flatten();
        let pane = &self.levels[self.focus];
        json!({
            "tree": {
                "phase": self.tree_load.phase,
                "request": self.tree_load.request,
                "entry_count": current.map(RepoTree::entry_count),
                "truncated": current.map(|tree| tree.truncated),
                "branch": current.map(|tree| &tree.branch),
                "error": self.tree_load.error,
                "displayed": self.tree.as_ref().zip(self.tree_source.as_ref())
                    .filter(|(_, request)| self.levels.len() > 2 && self.selected_repository().as_ref() == Some(&request.repository))
                    .map(|(tree, request)| json!({
                    "request": request,
                    "entry_count": tree.entry_count(),
                    "truncated": tree.truncated,
                    "branch": tree.branch,
                    "stale": self.tree_load.phase != LoadPhase::Ready || self.tree_request() != Some(request),
                })),
            },
            "owner_list": {
                "phase": self.owner_load.phase,
                "request": self.owner_load.request,
                "entry_count": self.owner_entry_count,
                "error": self.owner_load.error,
            },
            "pane": {
                "path": self.dir_path(),
                "depth": self.focus,
                "entry_count": pane.entries.len(),
                "visible_entry_count": pane.visible().len(),
            },
            "owner_kind": if self.selected_org().is_some() { "organization" } else { "unknown" },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_replacement_rejects_old_tree_and_owner_outcomes() {
        let mut browser = Browser::new(&["owner".into()]);
        let old_owner = browser.begin_owner_load("owner");
        browser.set_repo("owner", "project");
        let old_tree = browser.begin_tree_load("owner", "project");
        browser.reset_provider(&["owner".into()]);
        let owner = browser.begin_owner_load("owner");
        browser.org_repos_loaded(&old_owner, vec![RepoInfo::bare("obsolete")]);
        assert_eq!(browser.observation()["owner_list"]["phase"], "loading");
        browser.org_repos_loaded(&owner, vec![RepoInfo::bare("project")]);
        let current = browser.begin_tree_load("owner", "project");
        browser.tree_loaded(&old_tree, vec![], false, "obsolete".into());
        browser.tree_failed(&old_tree, &ProviderError::other("old provider failure"));
        assert_eq!(browser.observation()["tree"]["phase"], "loading");
        browser.tree_loaded(&current, vec![], false, "current".into());
        assert_eq!(browser.observation()["tree"]["phase"], "ready");
        assert_eq!(browser.observation()["tree"]["branch"], "current");
    }
}
