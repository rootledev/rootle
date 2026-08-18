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
}
