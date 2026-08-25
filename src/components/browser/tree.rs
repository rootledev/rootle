//! Repo tree queries: a fetched recursive tree reduced to the
//! miller-column questions the browser asks — "what are this path's
//! direct children?" and "where is this path's node?".

use crate::components::pane::{Entry, EntryKind};
use crate::provider::TreeNode;

/// A repo's full recursive tree, paths relative to the repo root.
pub struct RepoTree {
    pub owner: String,
    pub name: String,
    pub truncated: bool,
    /// Default branch (yank URLs, blob links).
    pub branch: String,
    entries: Vec<TreeNode>,
}

impl RepoTree {
    pub fn new(
        owner: String,
        name: String,
        truncated: bool,
        branch: String,
        entries: Vec<TreeNode>,
    ) -> Self {
        RepoTree {
            owner,
            name,
            truncated,
            branch,
            entries,
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(path: &str, is_dir: bool) -> TreeNode {
        TreeNode {
            path: path.into(),
            is_dir,
            sha: "sha".into(),
            size: Some(1),
        }
    }

    #[test]
    fn children_lists_direct_only_dirs_first() {
        let tree = RepoTree::new(
            "o".into(),
            "r".into(),
            false,
            "main".into(),
            vec![
                node("src", true),
                node("src/zeta.rs", false),
                node("src/alpha.rs", false),
                node("src/nested/deep.rs", false), // not a direct child
                node("src/widgets", true),
                node("zdocs.md", false),
                node("lib.rs", false),
            ],
        );
        let root: Vec<String> = tree.children("").iter().map(|e| e.name.clone()).collect();
        assert_eq!(root, vec!["src", "lib.rs", "zdocs.md"]);
        let src: Vec<String> = tree
            .children("src")
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert_eq!(src, vec!["widgets", "alpha.rs", "zeta.rs"]);
        assert!(tree.find("src/widgets").is_some());
    }
}
