//! Wire mapping: each `trait Provider` method as one NDJSON-RPC round
//! trip, with its reply shape. Wire structs live next to the method
//! that reads them — the shapes are the provider-protocol contract
//! (doc/provider-protocol.md), versioned in lockstep.

use super::StdioProvider;
use super::transport::{cancel_notification, de};
use crate::provider::{
    Capabilities, CodeMatch, Provider, ProviderError, ProviderResult, SearchCodeResult, SearchItem,
    TreeNode, TreeResult,
};
use serde_json::json;
use std::io::Write;
use std::sync::atomic::Ordering;

impl Provider for StdioProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn search(&self, query: &str) -> ProviderResult<Vec<SearchItem>> {
        #[derive(serde::Deserialize)]
        struct R {
            #[serde(default)]
            items: Vec<Item>,
        }
        #[derive(serde::Deserialize)]
        struct Item {
            full_name: Option<String>,
            org: Option<String>,
        }
        let r: R = de(self.request("search/repos", json!({ "query": query }))?)?;
        Ok(r.items
            .into_iter()
            .filter_map(|i| match (i.full_name, i.org) {
                (Some(r), _) => Some(SearchItem::Repo(r)),
                (None, Some(o)) => Some(SearchItem::Org(o)),
                _ => None,
            })
            .collect())
    }

    fn org_repos(&self, org: &str) -> ProviderResult<Vec<String>> {
        #[derive(serde::Deserialize)]
        struct R {
            #[serde(default)]
            repos: Vec<String>,
        }
        let r: R = de(self.request("org/repos", json!({ "org": org }))?)?;
        Ok(r.repos)
    }

    fn fetch_tree(&self, repo: &str) -> ProviderResult<TreeResult> {
        #[derive(serde::Deserialize)]
        struct R {
            #[serde(default)]
            entries: Vec<Entry>,
            #[serde(default)]
            truncated: bool,
            #[serde(default = "main")]
            branch: String,
        }
        #[derive(serde::Deserialize)]
        struct Entry {
            path: String,
            #[serde(rename = "type")]
            kind: String, // "blob" | "tree"
            sha: String,
            size: Option<u64>,
        }
        fn main() -> String {
            "main".into()
        }
        let r: R = de(self.request("repo/tree", json!({ "repo": repo }))?)?;
        Ok(TreeResult {
            entries: r
                .entries
                .into_iter()
                .map(|e| TreeNode {
                    path: e.path,
                    is_dir: e.kind == "tree",
                    sha: e.sha,
                    size: e.size,
                })
                .collect(),
            truncated: r.truncated,
            branch: r.branch,
        })
    }

    fn fetch_blob(&self, repo: &str, sha: &str) -> ProviderResult<Vec<u8>> {
        #[derive(serde::Deserialize)]
        struct R {
            bytes_b64: String,
        }
        let r: R = de(self.request("repo/blob", json!({ "repo": repo, "sha": sha }))?)?;
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(r.bytes_b64)
            .map_err(|e| ProviderError::other(format!("provider blob base64: {e}")))
    }

    fn web_url(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        line: Option<u32>,
        is_file: bool,
    ) -> ProviderResult<String> {
        #[derive(serde::Deserialize)]
        struct R {
            url: String,
        }
        let r: R = de(self.request(
            "repo/web_url",
            json!({ "repo": repo, "path": path, "branch": branch, "line": line, "is_file": is_file }),
        )?)?;
        Ok(r.url)
    }

    fn org_url(&self, org: &str) -> ProviderResult<String> {
        #[derive(serde::Deserialize)]
        struct R {
            url: String,
        }
        let r: R = de(self.request("org/url", json!({ "org": org }))?)?;
        Ok(r.url)
    }

    fn clone_url(&self, repo: &str) -> ProviderResult<String> {
        #[derive(serde::Deserialize)]
        struct R {
            clone_url: String,
        }
        let r: R = de(self.request("repo/clone_url", json!({ "repo": repo }))?)?;
        Ok(r.clone_url)
    }

    fn search_code(&self, q: &str) -> ProviderResult<SearchCodeResult> {
        let r: CodeReply = de(self.request("search/code", json!({ "q": q }))?)?;
        Ok(SearchCodeResult {
            hits: code_matches(&r.items),
            truncated: r.truncated,
        })
    }

    /// v1.3 progressive search (plans/0011): `partial: true` opts into
    /// `$/partial` batches for this request's id; the reply is
    /// metadata-only when the provider streamed (items empty,
    /// `truncated` authoritative).
    fn search_code_progressive(
        &self,
        q: &str,
        on_hits: &(dyn Fn(&[CodeMatch]) + Send + Sync),
    ) -> ProviderResult<SearchCodeResult> {
        let sink = |params: &serde_json::Value| {
            let items: Vec<WireItem> = params
                .get("items")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            on_hits(&code_matches(&items));
        };
        let reply: CodeReply = de(self.exchange_with_partials(
            "search/code",
            json!({ "q": q, "partial": true }),
            &sink,
        )?)?;
        Ok(SearchCodeResult {
            hits: Vec::new(),
            truncated: reply.truncated,
        })
    }

    /// v1.1 advisory cancel: name a request currently in flight, if
    /// any. Best-effort — a racing cancel for an id that just
    /// completed is ignored by the provider by contract.
    fn advise_cancel(&self) {
        let id = self.current_id.load(Ordering::Acquire);
        if id == 0 {
            return;
        }
        let line = cancel_notification(id);
        let mut process = self.process.lock();
        let _ = writeln!(process.stdin, "{line}");
        let _ = process.stdin.flush();
    }

    /// One-shot restart notice for the status line (plans/0008 §5).
    fn take_notice(&self) -> Option<String> {
        self.notice.lock().take()
    }

    /// Cache usage reported at the initialize handshake, when the
    /// provider participates in the advisory budget (v1.2).
    fn cache_usage(&self) -> Option<u64> {
        *self.cache_used.lock()
    }
}

/// `search/code` reply (items present only when the provider did not
/// stream) — the protocol v1.3 contract.
#[derive(serde::Deserialize)]
struct CodeReply {
    #[serde(default)]
    items: Vec<WireItem>,
    /// v1.2 (plans/0008 §4): provider capped its result set.
    #[serde(default)]
    truncated: bool,
}

#[derive(serde::Deserialize)]
struct WireItem {
    repo: String,
    path: String,
    #[serde(default)]
    sha: String,
    #[serde(default = "main_branch")]
    branch: String,
    #[serde(default)]
    matches: Vec<String>,
    /// v1.1: absent = located (verified placement).
    #[serde(default = "located")]
    located: bool,
}

fn main_branch() -> String {
    "main".into()
}

fn located() -> bool {
    true
}

fn code_matches(items: &[WireItem]) -> Vec<CodeMatch> {
    items
        .iter()
        .map(|i| CodeMatch {
            repo: i.repo.clone(),
            path: i.path.clone(),
            sha: i.sha.clone(),
            branch: i.branch.clone(),
            matches: i.matches.clone(),
            located: i.located,
        })
        .collect()
}
#[cfg(test)]
mod tests {
    /// The v1.1 `located` default: absent means located (verified
    /// placement); only an explicit false flags a stale hit.
    #[test]
    fn located_defaults_true_and_parses_false() {
        #[derive(serde::Deserialize)]
        struct Item {
            #[serde(default = "located")]
            located: bool,
        }
        fn located() -> bool {
            true
        }
        let absent: Item = serde_json::from_str("{}").unwrap();
        assert!(absent.located);
        let stale: Item = serde_json::from_str(r#"{"located":false}"#).unwrap();
        assert!(!stale.located);
    }
}
