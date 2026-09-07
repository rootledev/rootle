//! Persistent selection identities use source entities, never pane captions.

use super::{Browser, Entry, EntryKind};
use rootle_provider::id::OrgId;
use rootle_provider::{RepoId, RepoPath};
use std::collections::HashSet;

const ORGANIZATION_COLUMN: usize = 0;
const TREE_COLUMN_START: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MarkKey {
    Organization { organization: OrgId },
    Repository { repository: RepoId },
    Entry { repository: RepoId, path: RepoPath },
}

impl Browser {
    pub fn enter_visual(&mut self) {
        self.visual = true;
        self.sync();
    }
    pub fn exit_visual(&mut self) {
        self.visual = false;
        self.sync();
    }

    pub fn toggle_selected(&mut self) {
        let Some(entry) = self.levels[self.focus].selected_entry() else {
            return;
        };
        let Some(key) = self.mark_key(self.focus, entry) else {
            return;
        };
        if !self.marks.remove(&key) {
            self.marks.insert(key);
        }
        self.sync();
    }

    pub fn clear_marks(&mut self) {
        self.marks.clear();
        self.sync();
    }

    pub fn delete_marked_orgs(&mut self) -> Vec<String> {
        let deleted: Vec<_> = self
            .marks
            .iter()
            .filter_map(|mark| match mark {
                MarkKey::Organization { organization } => Some(organization.as_str().to_string()),
                _ => None,
            })
            .collect();
        if deleted.is_empty() {
            return deleted;
        }
        let selected_org = self.selected_org();
        self.levels[ORGANIZATION_COLUMN]
            .entries
            .retain(|entry| !deleted.contains(&entry.name));
        self.marks.retain(|mark| !matches!(mark, MarkKey::Organization { organization } if deleted.iter().any(|name| name == organization.as_str())));
        let selected = self.levels[ORGANIZATION_COLUMN]
            .entries
            .iter()
            .position(|entry| Some(entry.name.as_str()) == selected_org.as_deref())
            .unwrap_or(0);
        self.levels[ORGANIZATION_COLUMN].select(selected);
        if self.selected_org() != selected_org {
            self.levels.truncate(1);
            self.focus = ORGANIZATION_COLUMN;
            self.tree = None;
            self.set_current_ref(None);
        }
        self.sync();
        deleted
    }

    pub fn visual_marks(&self) -> Vec<MarkKey> {
        let mut marks: Vec<_> = self.marks.iter().cloned().collect();
        marks.sort();
        marks
    }

    pub(super) fn marked_names(&self, column: usize) -> HashSet<String> {
        self.levels[column]
            .entries
            .iter()
            .filter(|entry| {
                self.mark_key(column, entry)
                    .is_some_and(|key| self.marks.contains(&key))
            })
            .map(|entry| entry.name.clone())
            .collect()
    }

    fn mark_key(&self, column: usize, entry: &Entry) -> Option<MarkKey> {
        match entry.kind {
            EntryKind::Org => Some(MarkKey::Organization {
                organization: entry.name.as_str().into(),
            }),
            EntryKind::Repo => Some(MarkKey::Repository {
                repository: format!("{}/{}", self.selected_org()?, entry.name).into(),
            }),
            EntryKind::Dir | EntryKind::File => {
                let (owner, name) = self.repo_coords()?;
                let mut parts: Vec<&str> = self
                    .levels
                    .iter()
                    .take(column)
                    .skip(TREE_COLUMN_START)
                    .filter_map(|pane| pane.selected_entry().map(|entry| entry.name.as_str()))
                    .collect();
                parts.push(&entry.name);
                Some(MarkKey::Entry {
                    repository: format!("{owner}/{name}").into(),
                    path: parts.join("/").into(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_in_an_org_named_orgs_is_not_an_organization_mark() {
        let mut browser = Browser::new(&["orgs".into()], &[]);
        browser.org_repos_loaded("orgs", vec![rootle_provider::RepoInfo::bare("project")]);
        browser.toggle_selected();
        assert!(browser.delete_marked_orgs().is_empty());
        assert_eq!(browser.selected_org().as_deref(), Some("orgs"));
        assert!(
            matches!(&browser.visual_marks()[0], MarkKey::Repository { repository } if repository.as_str() == "orgs/project")
        );
    }
}
