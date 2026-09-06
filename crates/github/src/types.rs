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
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub pushed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RepoMeta {
    pub default_branch: String,
}

/// GET /search/code (Accept: application/vnd.github.text-match+json).
#[derive(Debug, Deserialize)]
pub struct SearchCodeResponse {
    #[serde(default)]
    pub total_count: u64,
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

/// GET /repos/{o}/{r}/commits/{sha} (v1.6, plans/0028): the commit
/// viewer's detail. `status` stays raw — GitHub also emits
/// "changed"/"unchanged", which the provider seam degrades to
/// `modified`.
#[derive(Debug, Deserialize)]
pub struct CommitResponse {
    pub sha: String,
    pub commit: CommitMeta,
    #[serde(default)]
    pub parents: Vec<CommitParent>,
    #[serde(default)]
    pub files: Vec<CommitFileItem>,
}

#[derive(Debug, Deserialize)]
pub struct CommitMeta {
    pub message: String,
    pub author: Option<CommitAuthor>,
}

#[derive(Debug, Deserialize)]
pub struct CommitAuthor {
    pub name: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommitParent {
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct CommitFileItem {
    pub filename: String,
    #[serde(default)]
    pub status: Option<String>,
    /// Absent when GitHub truncates very large diffs.
    #[serde(default)]
    pub additions: Option<u32>,
    #[serde(default)]
    pub deletions: Option<u32>,
    pub patch: Option<String>,
    #[serde(default)]
    pub previous_filename: Option<String>,
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

    #[test]
    fn parses_commit_payload() {
        let json = r#"{
            "sha": "6dcb09b5",
            "commit": {
                "message": "Fix the race\n\nBody line.",
                "author": {"name": "octocat", "date": "2026-09-06T10:00:00Z"}
            },
            "parents": [{"sha": "aaa111"}, {"sha": "bbb222"}],
            "files": [
                {"filename": "src/lib.rs", "status": "modified", "additions": 2,
                 "deletions": 1, "patch": "@@ -1 +1,2 @@\n-old\n+new\n+line"},
                {"filename": "src/old.rs", "status": "removed", "additions": 0, "deletions": 9},
                {"filename": "src/moved.rs", "status": "renamed", "previous_filename": "src/orig.rs",
                 "additions": 1, "deletions": 1},
                {"filename": "img/logo.png", "status": "changed", "additions": 3, "deletions": 0}
            ]
        }"#;
        let parsed: CommitResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.commit.message, "Fix the race\n\nBody line.");
        assert_eq!(
            (
                parsed.commit.author.as_ref().unwrap().name.as_deref(),
                parsed.commit.author.as_ref().unwrap().date.as_deref()
            ),
            (Some("octocat"), Some("2026-09-06T10:00:00Z"))
        );
        assert_eq!(parsed.parents.len(), 2);
        assert_eq!(parsed.parents[1].sha, "bbb222");
        assert_eq!(
            parsed.files[0].patch.as_deref(),
            Some("@@ -1 +1,2 @@\n-old\n+new\n+line")
        );
        assert_eq!(parsed.files[1].deletions, Some(9));
        assert_eq!(
            parsed.files[2].previous_filename.as_deref(),
            Some("src/orig.rs")
        );
        // Binary/no-patch files: patch absent, status stays raw wire
        // ("changed" is not one of the seam's four).
        assert_eq!(parsed.files[3].patch, None);
        assert_eq!(parsed.files[3].status.as_deref(), Some("changed"));
    }
}
