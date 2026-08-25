//! Stdio provider (plans/0005): an external child process speaking
//! newline-delimited JSON-RPC 2.0 on stdin/stdout — the LSP model.
//! Wrap any internal source-control system with a small script
//! (see examples/providers/) and point `[provider] command` at it.
//!
//! Layout: this module owns the provider lifecycle (spawn, handshake,
//! respawn-with-backoff); `transport.rs` owns the process + reader
//! thread + reply routing; `wire.rs` maps `trait Provider` methods
//! onto round trips.
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

use super::{Capabilities, ErrorKind, ProviderError, ProviderResult};
use serde_json::{Value, json};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

mod transport;
mod wire;

#[cfg(test)]
mod tests;

use transport::{Process, Shared, StderrMode, backoff_for, reader_loop, spawn_process};

/// Default per-request read deadline; `[provider] timeout_ms`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(30_000);

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
}
