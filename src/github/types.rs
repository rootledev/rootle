//! Serde models for the GitHub REST API (only what rootle consumes).
//! UI-facing types (`SearchItem`, `TreeNode`, …) live in
//! `crate::provider` — the trait boundary; these are wire models.

use serde::Deserialize;

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

/// GET /search/code (Accept: application/vnd.github.text-match+json).
#[derive(Debug, Deserialize)]
pub struct SearchCodeResponse {
    pub items: Vec<CodeItem>,
}

#[derive(Debug, Deserialize)]
pub struct CodeItem {
    pub path: String,
    pub sha: String,
    pub repository: CodeRepo,
    #[serde(default)]
    pub text_matches: Vec<TextMatch>,
}

#[derive(Debug, Deserialize)]
pub struct CodeRepo {
    pub full_name: String,
    #[serde(default)]
    pub default_branch: Option<String>,
}

/// A matched fragment: snippet text + match positions (byte indices
/// into `fragment`). Fragments carry no absolute line numbers — the
/// app locates them in the fetched blob for real line numbers.
#[derive(Debug, Deserialize)]
pub struct TextMatch {
    pub fragment: String,
    #[serde(default)]
    pub matches: Vec<MatchRange>,
}

#[derive(Debug, Deserialize)]
pub struct MatchRange {
    pub text: String,
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
        assert_eq!(parsed.tree[0].kind, "tree");
        assert_eq!(parsed.tree[1].size, Some(42));
    }
}
