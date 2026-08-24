//! Stdio provider (plans/0005): an external child process speaking
//! newline-delimited JSON-RPC 2.0 on stdin/stdout — the LSP model.
//! Wrap any internal source-control system with a small script
//! (see examples/providers/) and point `[provider] command` at it.
//!
//! Transport (plans/0008 §1): a dedicated reader thread owns the
//! child's stdout and routes replies by id into per-request slots.
//! Requests wait with a deadline (`[provider] timeout_ms`) — one hung
//! backend call fails instead of wedging every other call behind the
//! io mutex, and a late reply is discarded by id-matching. Child EOF
//! drops every pending slot, so a dead provider fails all in-flight
//! requests immediately.
//!
//! Restart (plans/0008 §5): EOF marks the transport dead; the next
//! request respawns the child with bounded backoff, re-runs the
//! initialize handshake, and only then proceeds. A successful restart
//! leaves a notice (`take_notice`) for the status line.

use super::{
    Capabilities, CodeMatch, ErrorKind, Provider, ProviderError, ProviderResult, SearchCodeResult,
    SearchItem, TreeNode, TreeResult,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

/// Default per-request read deadline; `[provider] timeout_ms`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(30_000);

/// Child stderr policy (plans/0008 §4).
#[derive(Clone, Copy)]
enum StderrMode {
    Null,
    Inherit,
}

/// Bounded respawn backoff (plans/0008 §5): 1s → 2s → 5s → 30s cap.
fn backoff_for(restart_attempt: u32) -> Duration {
    match restart_attempt {
        1 => Duration::from_secs(1),
        2 => Duration::from_secs(2),
        3 => Duration::from_secs(5),
        _ => Duration::from_secs(30),
    }
}

pub struct StdioProvider {
    name: String,
    capabilities: Capabilities,
    /// Child + its stdin, swapped on respawn. Writes take this lock:
    /// an advisory cancel (v1.1) must not queue behind another writer.
    process: Mutex<Process>,
    /// Id allocation + reply routing + liveness, shared with the
    /// reader thread(s).
    shared: Arc<Mutex<Shared>>,
    /// Request id currently in flight (0 = none) — advisory-cancel
    /// bookkeeping only; with pipelined requests it names one of them,
    /// which is fine (cancel is best-effort by contract).
    current_id: AtomicU64,
    /// Per-request read deadline (plans/0008 §1).
    timeout: Duration,
    /// Respawn parameters, kept verbatim from construction.
    command: Vec<String>,
    env: Vec<(String, String)>,
    stderr_mode: StderrMode,
    reader: Mutex<Option<JoinHandle<()>>>,
    /// One-shot UI notice (a successful restart), drained via
    /// `take_notice` (plans/0008 §5).
    notice: Mutex<Option<String>>,
}

struct Process {
    child: Child,
    stdin: ChildStdin,
}

#[derive(Default)]
struct Shared {
    next_id: u64,
    /// Reply slots by request id; the reader thread completes them.
    pending: HashMap<u64, mpsc::Sender<Value>>,
    /// Set by the reader thread on EOF; the next request respawns.
    dead: bool,
    /// Successful respawns so far (drives the backoff ladder).
    restarts: u32,
}

impl Drop for StdioProvider {
    /// rootle owns the provider lifecycle: the child dies with the app
    /// (kill first — stdin EOF alone is timing-dependent; the dead
    /// stdout then lets the reader thread exit and be joined).
    fn drop(&mut self) {
        if let Ok(process) = self.process.get_mut() {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
        if let Ok(mut reader) = self.reader.lock()
            && let Some(handle) = reader.take()
        {
            let _ = handle.join();
        }
    }
}

impl StdioProvider {
    /// Spawn the provider and run the initialize handshake.
    pub fn spawn(command: &[String], timeout: Duration) -> ProviderResult<Self> {
        Self::spawn_inner(command, timeout, &[], StderrMode::Null)
    }

    /// Spawn with the configured stderr policy (plans/0008 §4):
    /// `Inherit` passes the child's stderr through for adapter
    /// debugging; anything else discards it.
    pub fn spawn_with_stderr(
        command: &[String],
        timeout: Duration,
        inherit_stderr: bool,
    ) -> ProviderResult<Self> {
        let mode = if inherit_stderr {
            StderrMode::Inherit
        } else {
            StderrMode::Null
        };
        Self::spawn_inner(command, timeout, &[], mode)
    }

    /// Test-only variant: extra environment for the child process.
    #[cfg(test)]
    fn spawn_with_env(
        command: &[String],
        timeout: Duration,
        env: &[(&str, &str)],
    ) -> ProviderResult<Self> {
        Self::spawn_inner(command, timeout, env, StderrMode::Null)
    }

    fn spawn_inner(
        command: &[String],
        timeout: Duration,
        env: &[(&str, &str)],
        stderr_mode: StderrMode,
    ) -> ProviderResult<Self> {
        let (process, stdout) = spawn_process(command, env, stderr_mode)?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        let reader = std::thread::spawn({
            let shared = Arc::clone(&shared);
            move || reader_loop(stdout, shared)
        });
        let provider = StdioProvider {
            name: "stdio".into(),
            capabilities: Capabilities {
                orgs: true,
                code_search: true,
            },
            process: Mutex::new(process),
            shared,
            current_id: AtomicU64::new(0),
            timeout,
            command: command.to_vec(),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            stderr_mode,
            reader: Mutex::new(Some(reader)),
            notice: Mutex::new(None),
        };
        // Same deadline as any request: a provider that hangs on
        // startup fails into the github fallback instead of blocking
        // launch (plans/0008 §1).
        let reply = provider.handshake()?;
        provider.initialize_from(reply)
    }

    /// The v1 handshake. Also the liveness proof after a respawn.
    fn handshake(&self) -> ProviderResult<Value> {
        let reply = self.round_trip("initialize", json!({ "protocol": 1 }))?;
        let protocol = reply.get("protocol").and_then(Value::as_u64).unwrap_or(1);
        if protocol != 1 {
            return Err(ProviderError::new(
                ErrorKind::Provider,
                format!("unsupported provider protocol {protocol}"),
            ));
        }
        Ok(reply)
    }

    fn initialize_from(mut self, reply: Value) -> ProviderResult<Self> {
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

    /// One NDJSON-RPC round trip with liveness: respawn first if the
    /// reader thread reported EOF (plans/0008 §5).
    fn request(&self, method: &str, params: Value) -> ProviderResult<Value> {
        self.ensure_alive()?;
        self.round_trip(method, params)
    }

    /// Fast-path alive check; on death, one serialized respawn attempt
    /// per request. A failed attempt leaves the transport dead — the
    /// next request retries on a longer backoff, so a backend that
    /// recovers (fresh credentials, network back) rejoins on its own.
    fn ensure_alive(&self) -> ProviderResult<()> {
        if !self.shared.lock().expect("provider shared poisoned").dead {
            return Ok(());
        }
        self.respawn()
    }

    fn respawn(&self) -> ProviderResult<()> {
        let mut shared = self.shared.lock().expect("provider shared poisoned");
        if !shared.dead {
            return Ok(()); // another request beat us to it
        }
        let attempt = shared.restarts + 1;
        // Sleeping under the lock serializes concurrent callers into
        // one respawn — intended (plans/0008 §5).
        std::thread::sleep(backoff_for(attempt));
        // Reap the dead reader before replacing the process.
        if let Some(handle) = self.reader.lock().expect("reader poisoned").take() {
            let _ = handle.join();
        }
        let env: Vec<(&str, &str)> = self
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (process, stdout) = spawn_process(&self.command, &env, self.stderr_mode)?;
        let reader = std::thread::spawn({
            let shared = Arc::clone(&self.shared);
            move || reader_loop(stdout, shared)
        });
        *self.process.lock().expect("provider process poisoned") = process;
        *self.reader.lock().expect("reader poisoned") = Some(reader);
        shared.dead = false;
        shared.restarts = attempt;
        drop(shared);

        // The handshake is the liveness proof; on failure the
        // transport flips back to dead and the request fails here.
        if let Err(e) = self.handshake() {
            self.shared.lock().expect("provider shared poisoned").dead = true;
            return Err(e);
        }
        *self.notice.lock().expect("notice poisoned") = Some(format!(
            "provider restarted (attempt {attempt}, backoff {}s)",
            backoff_for(attempt).as_secs()
        ));
        Ok(())
    }

    /// One NDJSON-RPC round trip on a live transport: register a reply
    /// slot, write the request line, wait for the matched reply with a
    /// deadline. Timeout: the slot is removed and the (eventual) late
    /// reply is discarded by the reader — the transport stays usable.
    /// EOF: the reader dropped every slot, so the wait fails fast.
    fn round_trip(&self, method: &str, params: Value) -> ProviderResult<Value> {
        let (id, rx) = {
            let mut shared = self.shared.lock().expect("provider shared poisoned");
            shared.next_id += 1;
            let id = shared.next_id;
            let (tx, rx) = mpsc::channel::<Value>();
            shared.pending.insert(id, tx);
            (id, rx)
        };
        let line = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string();
        {
            let mut process = self.process.lock().expect("provider process poisoned");
            let written = writeln!(process.stdin, "{line}").and_then(|()| process.stdin.flush());
            if let Err(e) = written {
                self.shared
                    .lock()
                    .expect("provider shared poisoned")
                    .pending
                    .remove(&id);
                // A broken pipe almost certainly means a dead child —
                // the next request respawns (plans/0008 §5).
                self.shared.lock().expect("provider shared poisoned").dead = true;
                return Err(ProviderError::new(
                    ErrorKind::Provider,
                    format!("provider write: {e}"),
                ));
            }
        }
        let _cleared = CurrentIdGuard::new(self, id);

        let msg = match rx.recv_timeout(self.timeout) {
            Ok(msg) => msg,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.shared
                    .lock()
                    .expect("provider shared poisoned")
                    .pending
                    .remove(&id);
                return Err(ProviderError::new(
                    ErrorKind::Timeout,
                    format!("provider timeout after {}s", self.timeout.as_secs_f64()),
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(ProviderError::new(
                    ErrorKind::Provider,
                    "provider closed its output",
                ));
            }
        };
        if let Some(err) = msg.get("error") {
            // v1.1 taxonomy: semantics ride in data.kind; unknown or
            // absent kinds degrade to Other (plans/0008 §2).
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("provider error");
            let data = err.get("data");
            let kind = data
                .and_then(|d| d.get("kind"))
                .and_then(Value::as_str)
                .map(|k| match k {
                    "auth" => ErrorKind::Auth,
                    "rate_limited" => ErrorKind::RateLimited,
                    "not_found" => ErrorKind::NotFound,
                    "network" => ErrorKind::Network,
                    "timeout" => ErrorKind::Timeout,
                    "provider" => ErrorKind::Provider,
                    _ => ErrorKind::Other,
                })
                .unwrap_or(ErrorKind::Other);
            let error = ProviderError::new(kind, message);
            let retry = data
                .and_then(|d| d.get("retry_after_s"))
                .and_then(Value::as_u64);
            return Err(match retry {
                Some(s) => error.with_retry_after(Duration::from_secs(s)),
                None => error,
            });
        }
        msg.get("result")
            .cloned()
            .ok_or_else(|| ProviderError::other("provider reply without result"))
    }
}

/// Spawn the child and split its pipes for the process/reader halves.
fn spawn_process(
    command: &[String],
    env: &[(&str, &str)],
    stderr_mode: StderrMode,
) -> ProviderResult<(Process, ChildStdout)> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| ProviderError::other("empty provider command"))?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(match stderr_mode {
            StderrMode::Null => Stdio::null(),
            StderrMode::Inherit => Stdio::inherit(),
        });
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| ProviderError::other(format!("spawn {program}: {e}")))?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    Ok((Process { child, stdin }, stdout))
}

/// The stdout pump: read lines, complete the matching reply slot.
/// Id-less lines are tolerated chatter today and become the server-
/// initiated notification seam when that slice lands (plans/0008 §0).
/// On EOF or a fatal read error every pending slot is dropped
/// (failing all in-flight requests at once) and the transport is
/// marked dead so the next request respawns (plans/0008 §5).
fn reader_loop(stdout: ChildStdout, shared: Arc<Mutex<Shared>>) {
    let mut stdout = BufReader::new(stdout);
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = match stdout.read_line(&mut buf) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break; // EOF — child exited
        }
        let Ok(msg) = serde_json::from_str::<Value>(buf.trim()) else {
            continue; // tolerate non-JSON chatter
        };
        let Some(id) = msg.get("id").and_then(Value::as_u64) else {
            continue; // notification or reply without an id
        };
        let tx = shared
            .lock()
            .expect("provider shared poisoned")
            .pending
            .remove(&id);
        if let Some(tx) = tx {
            // A timed-out request has no slot; sending on a dropped
            // receiver is a no-op either way.
            let _ = tx.send(msg);
        }
    }
    let mut shared = shared.lock().expect("provider shared poisoned");
    shared.pending.clear();
    shared.dead = true;
}

/// Publishes `id` in `current_id` until the request leaves the wait
/// (reply, error, timeout, or closed pipe).
struct CurrentIdGuard<'a> {
    provider: &'a StdioProvider,
}

impl<'a> CurrentIdGuard<'a> {
    fn new(provider: &'a StdioProvider, id: u64) -> Self {
        provider.current_id.store(id, Ordering::Release);
        CurrentIdGuard { provider }
    }
}

impl Drop for CurrentIdGuard<'_> {
    fn drop(&mut self) {
        self.provider.current_id.store(0, Ordering::Release);
    }
}

/// The advisory-cancel notification line for a request id (v1.1).
fn cancel_notification(id: u64) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "$/cancelRequest",
        "params": { "id": id },
    })
    .to_string()
}

/// Deserialize a `result` value, tolerating missing optional fields.
fn de<T: serde::de::DeserializeOwned>(v: Value) -> ProviderResult<T> {
    serde_json::from_value(v)
        .map_err(|e| ProviderError::other(format!("provider reply shape: {e}")))
}

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
        #[derive(serde::Deserialize)]
        struct R {
            #[serde(default)]
            items: Vec<Item>,
            /// v1.2 (plans/0008 §4): provider capped its result set.
            #[serde(default)]
            truncated: bool,
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
            /// v1.1: absent = located (verified placement).
            #[serde(default = "located")]
            located: bool,
        }
        fn main() -> String {
            "main".into()
        }
        fn located() -> bool {
            true
        }
        let r: R = de(self.request("search/code", json!({ "q": q }))?)?;
        Ok(SearchCodeResult {
            hits: r
                .items
                .into_iter()
                .map(|i| CodeMatch {
                    repo: i.repo,
                    path: i.path,
                    sha: i.sha,
                    branch: i.branch,
                    matches: i.matches,
                    located: i.located,
                })
                .collect(),
            truncated: r.truncated,
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
        let Ok(mut process) = self.process.lock() else {
            return;
        };
        let _ = writeln!(process.stdin, "{line}");
        let _ = process.stdin.flush();
    }

    /// One-shot restart notice for the status line (plans/0008 §5).
    fn take_notice(&self) -> Option<String> {
        self.notice.lock().expect("notice poisoned").take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_notification_shape() {
        let line = cancel_notification(7);
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["method"], "$/cancelRequest");
        assert_eq!(v["params"]["id"], 7);
        assert!(v.get("id").is_none()); // notification, not request
    }

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

    // -- fake provider: this test binary re-executed as the child ----
    //
    // No python/sh dependency (the docker `test` stage has neither):
    // the parent spawns current_exe with a filter that runs ONLY the
    // `fake_provider_child` test below, which loops on stdin replying
    // per ROOTLE_FAKE_PROVIDER's script until killed.

    /// Child entry point — a no-op test in normal runs.
    #[test]
    fn fake_provider_child() {
        let Ok(mode) = std::env::var("ROOTLE_FAKE_PROVIDER") else {
            return;
        };
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        let mut handshaken = false;
        for line in stdin.lock().lines() {
            let line = line.expect("child read");
            let id: u64 = serde_json::from_str::<Value>(&line).unwrap()["id"]
                .as_u64()
                .unwrap_or(0);
            // The first message of every process generation is the
            // initialize handshake (respawns keep incrementing ids).
            if !handshaken {
                handshaken = true;
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocol":1,"name":"fake"}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
                continue;
            }
            match mode.as_str() {
                // Swallow id 2 without replying but keep serving later
                // requests — models a single lost/hung backend call.
                "hang-on-2" if id == 2 => {}
                "die-on-2" if id == 2 => std::process::exit(0),
                // Error taxonomy: each id answers with a differently
                // kinded error (plans/0008 §2).
                "error-kinds" => {
                    let (kind, extra) = match id {
                        2 => ("auth", ""),
                        3 => ("rate_limited", r#", "retry_after_s": 37"#),
                        4 => ("bogus-kind", ""),
                        _ => ("", ""),
                    };
                    if kind.is_empty() {
                        writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":1,"message":"plain"}}}}"#).unwrap();
                    } else {
                        writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":1,"message":"{kind} happened","data":{{"kind":"{kind}"{extra}}}}}}}"#).unwrap();
                    }
                    stdout.flush().unwrap();
                }
                "slow" => {
                    std::thread::sleep(Duration::from_millis(200));
                    writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#).unwrap();
                    stdout.flush().unwrap();
                }
                _ => {
                    writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#).unwrap();
                    stdout.flush().unwrap();
                }
            }
        }
    }

    /// Spawn the fake provider in `mode` with a short test deadline.
    /// The child is this test binary re-executed with a filter that
    /// runs only `fake_provider_child`; the mode travels via env.
    fn fake(mode: &str, timeout: Duration) -> StdioProvider {
        let exe = std::env::current_exe().expect("test binary path");
        let argv = vec![
            exe.to_string_lossy().into_owned(),
            "provider::stdio::tests::fake_provider_child".to_string(),
            "--exact".to_string(),
            "--nocapture".to_string(),
        ];
        StdioProvider::spawn_with_env(&argv, timeout, &[("ROOTLE_FAKE_PROVIDER", mode)])
            .expect("fake provider spawns + initializes")
    }

    #[test]
    fn timeout_fails_request_and_transport_recovers() {
        let provider = fake("hang-on-2", Duration::from_millis(300));
        let start = std::time::Instant::now();
        let err = provider
            .request("repo/tree", json!({ "repo": "o/r" }))
            .expect_err("id 2 must time out");
        assert_eq!(err.kind, ErrorKind::Timeout);
        assert!(
            err.message.contains("timeout"),
            "expected a timeout message, got: {err}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timeout must fire near the deadline, took {:?}",
            start.elapsed()
        );
        // The transport is unwedged: the next request (id 3) succeeds
        // even though id 2's reply never arrives.
        provider
            .request("org/repos", json!({ "org": "o" }))
            .expect("transport must recover after a timed-out request");
    }

    #[test]
    fn child_death_fails_pending_immediately() {
        let provider = fake("die-on-2", Duration::from_secs(30));
        let start = std::time::Instant::now();
        let err = provider
            .request("repo/tree", json!({ "repo": "o/r" }))
            .expect_err("dead child must fail the request");
        assert_eq!(
            err,
            ProviderError::new(ErrorKind::Provider, "provider closed its output")
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "EOF must fail fast, not at the deadline; took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn slow_replies_within_the_deadline_succeed() {
        let provider = fake("slow", Duration::from_secs(2));
        provider
            .request("org/repos", json!({ "org": "o" }))
            .expect("200ms reply must land within a 2s deadline");
    }

    #[test]
    fn error_data_kind_parses_into_the_taxonomy() {
        let provider = fake("error-kinds", Duration::from_secs(2));

        let err = provider.request("a", json!({})).expect_err("auth error");
        assert_eq!(err.kind, ErrorKind::Auth);
        assert_eq!(err.message, "auth happened");
        assert_eq!(err.retry_after, None);

        let err = provider.request("b", json!({})).expect_err("rate limited");
        assert_eq!(err.kind, ErrorKind::RateLimited);
        assert_eq!(err.retry_after, Some(Duration::from_secs(37)));

        // Unknown kinds degrade to Other; absent data does too.
        let err = provider.request("c", json!({})).expect_err("unknown kind");
        assert_eq!(err.kind, ErrorKind::Other);
        let err = provider.request("d", json!({})).expect_err("kindless");
        assert_eq!(err.kind, ErrorKind::Other);
        assert_eq!(err.message, "plain");
    }

    #[test]
    fn restart_recovers_after_child_death() {
        let provider = fake("die-on-2", Duration::from_secs(5));
        // Generation 1 dies serving id 2.
        provider
            .request("repo/tree", json!({ "repo": "o/r" }))
            .expect_err("dead child must fail the request");
        // The NEXT request respawns (1s backoff), re-handshakes, and
        // proceeds — id allocation keeps climbing, so the new child
        // never sees a colliding id.
        let start = std::time::Instant::now();
        provider
            .request("org/repos", json!({ "org": "o" }))
            .expect("respawn must recover the transport");
        assert!(
            start.elapsed() >= Duration::from_secs(1),
            "first restart pays the 1s backoff"
        );
        let notice = provider.take_notice().expect("restart leaves a notice");
        assert!(
            notice.contains("provider restarted"),
            "notice names the restart, got: {notice}"
        );
        assert!(provider.take_notice().is_none(), "notice is one-shot");
    }

    #[test]
    fn backoff_ladder() {
        assert_eq!(backoff_for(1), Duration::from_secs(1));
        assert_eq!(backoff_for(2), Duration::from_secs(2));
        assert_eq!(backoff_for(3), Duration::from_secs(5));
        assert_eq!(backoff_for(9), Duration::from_secs(30));
    }
}
