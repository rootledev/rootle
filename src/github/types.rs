//! Serde models for the GitHub REST API (only what ghx consumes).

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchItem {
    /// "owner/name"
    Repo(String),
    /// org login
    Org(String),
}

#[derive(Debug, Deserialize)]
pub struct SearchReposResponse {
    pub items: Vec<RepoItem>,
}

#[derive(Debug, Deserialize)]
pub struct RepoItem {
    pub full_name: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchUsersResponse {
    pub items: Vec<UserItem>,
}

#[derive(Debug, Deserialize)]
pub struct UserItem {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct OrgRepoItem {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct RepoMeta {
    pub default_branch: String,
}

/// GET /repos/{o}/{r}/git/trees/{branch}?recursive=1
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct TreeResponse {
    pub sha: String,
    #[serde(default)]
    pub truncated: bool,
    pub tree: Vec<TreeEntry>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct TreeEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String, // "blob" | "tree"
    pub sha: String,
    #[serde(default)]
    pub size: Option<u64>,
}

/// Internal, UI-facing node derived from a TreeEntry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub path: String,
    pub is_dir: bool,
    pub sha: String,
    pub size: Option<u64>,
}

impl From<&TreeEntry> for TreeNode {
    fn from(e: &TreeEntry) -> Self {
        TreeNode {
            path: e.path.clone(),
            is_dir: e.kind == "tree",
            sha: e.sha.clone(),
            size: e.size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_search_payload() {
        let json = r#"{"total_count": 1, "items": [{"full_name": "ratatui/ratatui"}]}"#;
        let parsed: SearchReposResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.items[0].full_name, "ratatui/ratatui");
    }

    #[test]
    fn parses_org_search_payload() {
        let json = r#"{"items": [{"login": "tokio-rs", "type": "Organization"}]}"#;
        let parsed: SearchUsersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.items[0].login, "tokio-rs");
    }

    #[test]
    fn parses_tree_payload() {
        let json = r#"{
            "sha": "abc123",
            "truncated": false,
            "tree": [
                {"path": "src", "mode": "040000", "type": "tree", "sha": "d1"},
                {"path": "src/lib.rs", "mode": "100644", "type": "blob", "sha": "f1", "size": 42}
            ]
        }"#;
        let parsed: TreeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.tree.len(), 2);
        let node = TreeNode::from(&parsed.tree[0]);
        assert!(node.is_dir);
        assert_eq!(node.path, "src");
        let file = TreeNode::from(&parsed.tree[1]);
        assert!(!file.is_dir);
        assert_eq!(file.size, Some(42));
    }
}
