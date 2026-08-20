//! Stdio provider (plans/0005): an external child process speaking
//! newline-delimited JSON-RPC 2.0 on stdin/stdout — the LSP model.
//! Wrap any internal source-control system with a small script
//! (see examples/providers/) and point `[provider] command` at it.
//!
//! One child per app, requests mutex-serialized, matched by id. No
//! restart policy in v1: a dead child surfaces errors as toasts.

use super::{Capabilities, CodeMatch, Provider, SearchItem, TreeNode, TreeResult};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

pub struct StdioProvider {
    name: String,
    capabilities: Capabilities,
    io: Mutex<Io>,
}

struct Io {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    child: Child,
    next_id: u64,
}

impl Drop for StdioProvider {
    /// ghx owns the provider lifecycle: the child dies with the app
    /// (kill first — stdin EOF alone is timing-dependent).
    fn drop(&mut self) {
        if let Ok(io) = self.io.get_mut() {
            let _ = io.child.kill();
            let _ = io.child.wait();
        }
    }
}

impl StdioProvider {
    /// Spawn the provider and run the initialize handshake.
    pub fn spawn(command: &[String]) -> Result<Self, String> {
        let (program, args) = command
            .split_first()
            .ok_or_else(|| "empty provider command".to_string())?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn {program}: {e}"))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let provider = StdioProvider {
            name: "stdio".into(),
            capabilities: Capabilities {
                orgs: true,
                code_search: true,
            },
            io: Mutex::new(Io {
                stdin,
                stdout,
                child,
                next_id: 0,
            }),
        };
        let caps = provider.initialize()?;
        Ok(caps)
    }

    fn initialize(mut self) -> Result<Self, String> {
        let reply = self.request("initialize", json!({ "protocol": 1 }))?;
        let protocol = reply.get("protocol").and_then(Value::as_u64).unwrap_or(1);
        if protocol != 1 {
            return Err(format!("unsupported provider protocol {protocol}"));
        }
        if let Some(name) = reply.get("name").and_then(Value::as_str) {
            self.name = format!("stdio:{name}");
        }
        if let Some(caps) = reply.get("capabilities") {
            self.capabilities = Capabilities {
                orgs: caps.get("orgs").and_then(Value::as_bool).unwrap_or(true),
                code_search: caps
                    .get("code_search")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            };
        }
        Ok(self)
    }

    /// One NDJSON-RPC round trip: write the request line, read lines
    /// until the matching id answers (skip anything else).
    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let io = &mut *self.io.lock().expect("provider io poisoned");
        io.next_id += 1;
        let id = io.next_id;
        let line = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string();
        writeln!(io.stdin, "{line}").map_err(|e| format!("provider write: {e}"))?;
        io.stdin
            .flush()
            .map_err(|e| format!("provider flush: {e}"))?;

        let mut buf = String::new();
        loop {
            buf.clear();
            let n = io
                .stdout
                .read_line(&mut buf)
                .map_err(|e| format!("provider read: {e}"))?;
            if n == 0 {
                return Err("provider closed its output".into());
            }
            let msg: Value = match serde_json::from_str(buf.trim()) {
                Ok(v) => v,
                Err(_) => continue, // tolerate non-JSON chatter
            };
            if msg.get("id").and_then(Value::as_u64) != Some(id) {
                continue; // notification or stale reply
            }
            if let Some(err) = msg.get("error") {
                let message = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("provider error");
                return Err(message.to_string());
            }
            return msg
                .get("result")
                .cloned()
                .ok_or_else(|| "provider reply without result".to_string());
        }
    }
}

/// Deserialize a `result` value, tolerating missing optional fields.
fn de<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("provider reply shape: {e}"))
}

impl Provider for StdioProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn search(&self, query: &str) -> Result<Vec<SearchItem>, String> {
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

    fn org_repos(&self, org: &str) -> Result<Vec<String>, String> {
        #[derive(serde::Deserialize)]
        struct R {
            #[serde(default)]
            repos: Vec<String>,
        }
        let r: R = de(self.request("org/repos", json!({ "org": org }))?)?;
        Ok(r.repos)
    }

    fn fetch_tree(&self, repo: &str) -> Result<TreeResult, String> {
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

    fn fetch_blob(&self, repo: &str, sha: &str) -> Result<Vec<u8>, String> {
        #[derive(serde::Deserialize)]
        struct R {
            bytes_b64: String,
        }
        let r: R = de(self.request("repo/blob", json!({ "repo": repo, "sha": sha }))?)?;
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(r.bytes_b64)
            .map_err(|e| format!("provider blob base64: {e}"))
    }

    fn web_url(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        line: Option<u32>,
        is_file: bool,
    ) -> Result<String, String> {
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

    fn org_url(&self, org: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct R {
            url: String,
        }
        let r: R = de(self.request("org/url", json!({ "org": org }))?)?;
        Ok(r.url)
    }

    fn clone_url(&self, repo: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct R {
            clone_url: String,
        }
        let r: R = de(self.request("repo/clone_url", json!({ "repo": repo }))?)?;
        Ok(r.clone_url)
    }

    fn search_code(&self, q: &str) -> Result<Vec<CodeMatch>, String> {
        #[derive(serde::Deserialize)]
        struct R {
            #[serde(default)]
            items: Vec<Item>,
        }
        #[derive(serde::Deserialize)]
        struct Item {
            repo: String,
            path: String,
            #[serde(default)]
            sha: String,
            #[serde(default = "main")]
            branch: String,
            #[serde(default)]
            matches: Vec<String>,
        }
        fn main() -> String {
            "main".into()
        }
        let r: R = de(self.request("search/code", json!({ "q": q }))?)?;
        Ok(r.items
            .into_iter()
            .map(|i| CodeMatch {
                repo: i.repo,
                path: i.path,
                sha: i.sha,
                branch: i.branch,
                matches: i.matches,
            })
            .collect())
    }
}
