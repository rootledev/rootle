//! Repository, organization and code search.

use super::{API, GitHubClient, urlencoding};
use crate::types::{OrgRepoItem, SearchReposResponse, SearchUsersResponse};
use rootle_provider::{ErrorKind, ProviderError, ProviderResult, SearchItem};

impl GitHubClient {
    /// Repo search + org search, merged: orgs first, then repos.
    /// Returns provider-level items (the trait boundary type).
    pub fn search(&self, query: &str) -> ProviderResult<Vec<SearchItem>> {
        let q = urlencoding(query);
        let mut out = Vec::new();

        let orgs: SearchUsersResponse =
            self.get(&format!("{API}/search/users?q={q}+type:org&per_page=5"))?;
        out.extend(orgs.items.into_iter().map(|u| SearchItem::Org(u.login)));

        let repos: SearchReposResponse =
            self.get(&format!("{API}/search/repositories?q={q}&per_page=20"))?;
        out.extend(
            repos
                .items
                .into_iter()
                .map(|r| SearchItem::Repo(r.full_name)),
        );

        Ok(out)
    }

    /// The legacy method name covers explicit owner listing, not just organizations.
    pub fn org_repos(&self, owner: &str) -> ProviderResult<Vec<rootle_provider::RepoInfo>> {
        self.owner_repos_at(API, owner)
    }

    fn owner_repos_at(
        &self,
        api: &str,
        owner: &str,
    ) -> ProviderResult<Vec<rootle_provider::RepoInfo>> {
        #[derive(serde::Deserialize)]
        struct Account {
            #[serde(rename = "type")]
            kind: String,
        }
        let owner = urlencoding(owner);
        let account: Account = self.get(&format!("{api}/users/{owner}"))?;
        let collection = match account.kind.as_str() {
            "Organization" => "orgs",
            "User" | "Bot" => "users",
            _ => {
                return Err(ProviderError::new(
                    ErrorKind::Provider,
                    "unsupported GitHub account type",
                ));
            }
        };
        let repos: Vec<OrgRepoItem> = self.get(&format!(
            "{api}/{collection}/{owner}/repos?per_page=100&sort=updated"
        ))?;
        Ok(repos
            .into_iter()
            .map(|r| rootle_provider::RepoInfo {
                name: r.name,
                description: r.description,
                private: r.private,
                archived: r.archived,
                pushed_at: r.pushed_at,
            })
            .collect())
    }

    /// Code search (plans/0002 §4). Requires auth — anonymous clients
    /// get a clear error. `q` is the full query string including
    /// qualifiers (`repo:`, `org:`, `extension:`, `path:`); text-match
    /// fragments are requested for previews.
    pub fn search_code(&self, q: &str) -> ProviderResult<(Vec<crate::types::CodeItem>, bool)> {
        let (items, total) = self.search_code_page(q, 1)?;
        // GitHub caps code search at 1000 results — that is the
        // provider's own truncation signal (plans/0008 §4).
        Ok((items, total > 1000))
    }

    /// One page of code search (`per_page=100`, 1-indexed pages).
    /// Returns (items, total_count) — total drives `truncated` and the
    /// progressive page loop (v1.3, plans/0011).
    pub fn search_code_page(
        &self,
        q: &str,
        page: u32,
    ) -> ProviderResult<(Vec<crate::types::CodeItem>, u64)> {
        if self.is_anonymous() {
            return Err(ProviderError::new(
                ErrorKind::Auth,
                "code search needs a token — set ROOTLE_TOKEN or log in with `gh`",
            ));
        }
        let url = format!(
            "{API}/search/code?q={}&per_page=100&page={page}",
            urlencoding(q)
        );
        let resp: crate::types::SearchCodeResponse =
            self.get_accept(&url, "application/vnd.github.text-match+json")?;
        Ok((resp.items, resp.total_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn server(
        responses: Vec<(&'static str, u16, &'static str)>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for (path, status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                assert_eq!(request.split_whitespace().nth(1), Some(path));
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (address, worker)
    }

    #[test]
    fn explicit_owner_listing_uses_account_type_and_preserves_metadata() {
        for (kind, path) in [
            (
                "{\"type\":\"User\"}",
                "/users/owner/repos?per_page=100&sort=updated",
            ),
            (
                "{\"type\":\"Organization\"}",
                "/orgs/owner/repos?per_page=100&sort=updated",
            ),
        ] {
            let (api, worker) = server(vec![
                ("/users/owner", 200, kind),
                (
                    path,
                    200,
                    "[{\"name\":\"project\",\"private\":true,\"archived\":true,\"description\":\"kept\"}]",
                ),
            ]);
            let repos = GitHubClient::anonymous()
                .owner_repos_at(&api, "owner")
                .unwrap();
            worker.join().unwrap();
            assert_eq!(
                repos,
                vec![rootle_provider::RepoInfo {
                    name: "project".into(),
                    private: true,
                    archived: true,
                    description: Some("kept".into()),
                    pushed_at: None,
                }]
            );
        }
    }

    #[test]
    fn missing_owner_is_not_retried_as_another_account_type() {
        let (api, worker) = server(vec![("/users/owner", 404, "{}")]);
        let error = GitHubClient::anonymous()
            .owner_repos_at(&api, "owner")
            .unwrap_err();
        worker.join().unwrap();
        assert_eq!(error.kind, ErrorKind::NotFound);
    }

    #[test]
    fn listing_auth_error_remains_an_auth_error() {
        let (api, worker) = server(vec![
            ("/users/owner", 200, "{\"type\":\"User\"}"),
            ("/users/owner/repos?per_page=100&sort=updated", 401, "{}"),
        ]);
        let error = GitHubClient::anonymous()
            .owner_repos_at(&api, "owner")
            .unwrap_err();
        worker.join().unwrap();
        assert_eq!(error.kind, ErrorKind::Auth);
    }
}
