//! Transport machinery (plans/0008 §1): the child process, the stdout
//! reader thread, and the reply-routing round trip. Nothing here knows
//! the provider method surface — `wire.rs` maps methods onto
//! `round_trip`.

use super::StdioProvider;
use crate::provider::{ErrorKind, ProviderError, ProviderResult};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, mpsc};
use std::time::Duration;

/// Child stderr policy (plans/0008 §4).
#[derive(Clone, Copy)]
pub(super) enum StderrMode {
    Null,
    Inherit,
}

/// Bounded respawn backoff (plans/0008 §5): 1s → 2s → 5s → 30s cap.
pub(super) fn backoff_for(restart_attempt: u32) -> Duration {
    match restart_attempt {
        1 => Duration::from_secs(1),
        2 => Duration::from_secs(2),
        3 => Duration::from_secs(5),
        _ => Duration::from_secs(30),
    }
}

pub(super) struct Process {
    pub(super) child: Child,
    pub(super) stdin: ChildStdin,
}

#[derive(Default)]
pub(super) struct Shared {
    pub(super) next_id: u64,
    /// Reply slots by request id; the reader thread completes them.
    pub(super) pending: HashMap<u64, mpsc::Sender<Value>>,
    /// Set by the reader thread on EOF; the next request respawns.
    pub(super) dead: bool,
    /// Successful respawns so far (drives the backoff ladder).
    pub(super) restarts: u32,
}

impl StdioProvider {
    /// One NDJSON-RPC round trip on a live transport: register a reply
    /// slot, write the request line, wait for the matched reply with a
    /// deadline. Timeout: the slot is removed and the (eventual) late
    /// reply is discarded by the reader — the transport stays usable.
    /// EOF: the reader dropped every slot, so the wait fails fast.
    pub(super) fn round_trip(&self, method: &str, params: Value) -> ProviderResult<Value> {
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
            return Err(error_from_reply(err));
        }
        msg.get("result")
            .cloned()
            .ok_or_else(|| ProviderError::other("provider reply without result"))
    }
}

/// The v1.1 error taxonomy: semantics ride in `data.kind`; unknown or
/// absent kinds degrade to Other (plans/0008 §2).
fn error_from_reply(err: &Value) -> ProviderError {
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
    match data
        .and_then(|d| d.get("retry_after_s"))
        .and_then(Value::as_u64)
    {
        Some(s) => error.with_retry_after(Duration::from_secs(s)),
        None => error,
    }
}

/// Spawn the child and split its pipes for the process/reader halves.
pub(super) fn spawn_process(
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
pub(super) fn reader_loop(stdout: ChildStdout, shared: Arc<Mutex<Shared>>) {
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
pub(super) fn cancel_notification(id: u64) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "$/cancelRequest",
        "params": { "id": id },
    })
    .to_string()
}

/// Deserialize a `result` value, tolerating missing optional fields.
pub(super) fn de<T: serde::de::DeserializeOwned>(v: Value) -> ProviderResult<T> {
    serde_json::from_value(v)
        .map_err(|e| ProviderError::other(format!("provider reply shape: {e}")))
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
    fn backoff_ladder() {
        assert_eq!(backoff_for(1), Duration::from_secs(1));
        assert_eq!(backoff_for(2), Duration::from_secs(2));
        assert_eq!(backoff_for(3), Duration::from_secs(5));
        assert_eq!(backoff_for(9), Duration::from_secs(30));
    }
}
