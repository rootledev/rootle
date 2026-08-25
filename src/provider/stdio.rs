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
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

mod transport;
mod wire;

#[cfg(test)]
mod tests;

use transport::{Lifecycle, Process, Shared, StderrMode, backoff_for, reader_loop, spawn_process};

/// Default per-request read deadline; `[provider] timeout_ms`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(30_000);

pub struct StdioProvider {
    name: String,
    capabilities: Capabilities,
    /// Child + its stdin, swapped on rebuild. Writes take this lock:
    /// an advisory cancel (v1.1) must not queue behind another writer.
    process: Mutex<Process>,
    /// Id allocation + reply routing + lifecycle, shared with the
    /// reader thread(s).
    shared: Arc<Shared>,
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
    /// One-shot UI notice (a successful restart, a config warning),
    /// drained via `take_notice` (plans/0008 §5).
    notice: Mutex<Option<String>>,
    /// Set by Drop: a rebuild sleeping in backoff checks this before
    /// spawning a replacement, so dropping the provider mid-recovery
    /// can't leak an orphan child after the app is gone. Drop can't
    /// race an in-flight rebuild only because every worker clones the
    /// `Arc<dyn Provider>` before moving into its thread (the strong
    /// count can't hit zero mid-call) — keep that convention.
    closed: AtomicBool,
    /// Advisory cache budget (the user's [cache] max_mb in bytes) and
    /// this provider's subtree path — passed at every initialize;
    /// providers that cache SHOULD evict past it (protocol v1.2).
    cache_bytes: u64,
    cache_dir: Option<std::path::PathBuf>,
    /// Cache usage the provider reported at initialize, if any —
    /// surfaced in :settings.
    cache_used: parking_lot::Mutex<Option<u64>>,
}

impl Drop for StdioProvider {
    /// rootle owns the provider lifecycle: the child dies with the app
    /// (kill first — stdin EOF alone is timing-dependent; the dead
    /// stdout then lets the reader thread exit and be joined).
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        let process = self.process.get_mut();
        let _ = process.child.kill();
        let _ = process.child.wait();
        if let Some(handle) = self.reader.lock().take() {
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
        Self::spawn_with_cache(command, timeout, inherit_stderr, 0, None)
    }

    /// Spawn with the user's cache budget and the provider's subtree
    /// path — both travel in every initialize (protocol v1.2, advisory).
    pub fn spawn_with_cache(
        command: &[String],
        timeout: Duration,
        inherit_stderr: bool,
        cache_bytes: u64,
        cache_dir: Option<std::path::PathBuf>,
    ) -> ProviderResult<Self> {
        let mode = if inherit_stderr {
            StderrMode::Inherit
        } else {
            StderrMode::Null
        };
        let mut provider = Self::spawn_inner(command, timeout, &[], mode)?;
        provider.cache_bytes = cache_bytes;
        provider.cache_dir = cache_dir;
        // The initial handshake already ran inside spawn_inner without
        // the budget; re-run it so the provider hears cache_bytes on
        // THIS generation too. Respawns always carry it.
        let reply = provider.handshake()?;
        provider.initialize_from(reply)
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
        let shared = Arc::new(Shared::default());
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
            closed: AtomicBool::new(false),
            cache_bytes: 0,
            cache_dir: None,
            cache_used: parking_lot::Mutex::new(None),
        };
        // Same deadline as any request: a provider that hangs on
        // startup fails into the github fallback instead of blocking
        // launch (plans/0008 §1).
        let reply = provider.handshake()?;
        provider.initialize_from(reply)
    }

    /// The v1 handshake. Also the liveness proof after a rebuild: it
    /// runs ungated so the rebuilder can validate the fresh child
    /// while the transport sits in `Respawning`.
    fn handshake(&self) -> ProviderResult<Value> {
        let mut params = json!({ "protocol": 1 });
        if self.cache_bytes > 0 {
            params["cache_bytes"] = json!(self.cache_bytes);
        }
        if let Some(dir) = &self.cache_dir {
            params["cache_dir"] = json!(dir.to_string_lossy());
        }
        let reply = self.exchange("initialize", params, false)?;
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
        if let Some(bytes) = reply.pointer("/cache/bytes").and_then(Value::as_u64) {
            *self.cache_used.lock() = Some(bytes);
        }
        Ok(self)
    }

    /// One NDJSON-RPC round trip with liveness: rebuild first if the
    /// reader thread reported EOF (plans/0008 §5).
    fn request(&self, method: &str, params: Value) -> ProviderResult<Value> {
        self.ensure_alive()?;
        self.exchange(method, params, true)
    }

    /// The recovery gate. `Alive` proceeds; the first caller to see
    /// `Dead` becomes the rebuilder (state → `Respawning`, lock
    /// released before any sleeping); callers arriving while a rebuild
    /// is in flight wait on the condvar — the mutex stays free, so id
    /// allocation and reply routing never queue behind a backoff
    /// sleep. A waiter that wakes to `Dead` fails immediately with the
    /// attempt's stored error: at most ONE caller ever pays for a
    /// given rebuild attempt, and nobody chains into a second sleep.
    /// A fresh caller arriving after a failed attempt starts the next
    /// one on the longer backoff — a backend that recovers (fresh
    /// credentials, network back) rejoins on its own.
    fn ensure_alive(&self) -> ProviderResult<()> {
        let mut routing = self.shared.routing.lock();
        let mut waited = false;
        loop {
            match routing.lifecycle {
                Lifecycle::Alive => return Ok(()),
                Lifecycle::Respawning => {
                    waited = true;
                    // Reacquires `routing` on wake; the loop re-reads
                    // the state (spurious wakeups are fine).
                    self.shared.changed.wait(&mut routing);
                }
                Lifecycle::Dead if waited => {
                    let reason = routing.restart_error.clone();
                    drop(routing);
                    return Err(match reason {
                        Some(why) => ProviderError::new(
                            ErrorKind::Provider,
                            format!("provider restart failed: {why}"),
                        ),
                        None => ProviderError::new(
                            ErrorKind::Provider,
                            "provider restarting — try again",
                        ),
                    });
                }
                Lifecycle::Dead => {
                    routing.lifecycle = Lifecycle::Respawning;
                    routing.restart_error = None;
                    let attempt = routing.restarts + 1;
                    drop(routing);
                    return self.rebuild(attempt);
                }
            }
        }
    }

    /// Rebuild the child after transport death: backoff sleep, reap
    /// the dead reader, spawn the replacement, then prove it with the
    /// v1 handshake — only a validated child flips the transport back
    /// to `Alive`, so no request ever runs against an unproven
    /// process. Runs with no locks held; `Respawning` (set by the
    /// caller) makes other threads wait rather than pile onto the
    /// mutex.
    fn rebuild(&self, attempt: u32) -> ProviderResult<()> {
        // Armed for the whole body: if any path out of here panics
        // (the reader `thread::spawn` under thread exhaustion) or a
        // future exit path forgets to publish, the guard's Drop
        // publishes a failed rebuild — "Respawning always resolves"
        // holds structurally, not by enumerating exits (the spawn
        // `?` bug was one branch out of three forgetting).
        let mut guard = RebuildGuard {
            provider: self,
            attempt,
            armed: true,
        };
        std::thread::sleep(backoff_for(attempt));
        if self.closed.load(Ordering::Acquire) {
            // Dropped mid-recovery (app exit): don't spawn a child
            // nobody will ever kill. Still publish the outcome so any
            // waiter wakes and fails instead of waiting forever.
            let err = ProviderError::other("provider dropped during restart");
            guard.armed = false;
            self.finish_rebuild(attempt, Err(&err));
            return Err(err);
        }
        // Reap the dead reader before replacing the process.
        if let Some(handle) = self.reader.lock().take() {
            let _ = handle.join();
        }
        let env: Vec<(&str, &str)> = self
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (process, stdout) = match spawn_process(&self.command, &env, self.stderr_mode) {
            Ok(pair) => pair,
            Err(e) => {
                // A failed spawn is a failed rebuild like any other:
                // publish it and wake the waiters, or the machine
                // wedges in Respawning and every future caller parks
                // on a condvar nobody will ever signal (one error
                // toast, then an app that silently fetches nothing).
                guard.armed = false;
                self.finish_rebuild(attempt, Err(&e));
                return Err(e);
            }
        };
        // Panics here (thread exhaustion) unwind past everything —
        // the guard publishes.
        let reader = std::thread::spawn({
            let shared = Arc::clone(&self.shared);
            move || reader_loop(stdout, shared)
        });
        *self.process.lock() = process;
        *self.reader.lock() = Some(reader);

        // The handshake is the liveness proof. On failure the
        // transport returns to Dead; the next fresh request retries on
        // the longer backoff.
        let proof = self.handshake();
        if proof.is_ok() {
            *self.notice.lock() = Some(format!(
                "provider restarted (attempt {attempt}, backoff {}s)",
                backoff_for(attempt).as_secs()
            ));
        }
        let outcome: Result<(), &ProviderError> = proof.as_ref().map(|_| ());
        guard.armed = false;
        self.finish_rebuild(attempt, outcome);
        proof.map(|_| ())
    }

    /// Publish a rebuild's outcome under the lock and wake every
    /// waiter.
    fn finish_rebuild(&self, attempt: u32, outcome: Result<(), &ProviderError>) {
        let mut routing = self.shared.routing.lock();
        match outcome {
            Ok(()) => {
                routing.lifecycle = Lifecycle::Alive;
                routing.restarts = attempt;
                routing.restart_error = None;
            }
            Err(e) => {
                routing.lifecycle = Lifecycle::Dead;
                routing.restart_error = Some(e.message.clone());
            }
        }
        drop(routing);
        self.shared.changed.notify_all();
    }
}

/// The structural half of the recovery machine: while armed, a Drop
/// publishes a failed rebuild. `rebuild` disarms exactly around its
/// `finish_rebuild` calls; anything that leaves the function any other
/// way — an early return that forgets, a panic mid-body — still
/// resolves `Respawning`, so no waiter can park on a condvar nobody
/// will ever signal.
struct RebuildGuard<'a> {
    provider: &'a StdioProvider,
    attempt: u32,
    armed: bool,
}

impl Drop for RebuildGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let err = ProviderError::other("rebuild aborted unexpectedly");
            self.provider.finish_rebuild(self.attempt, Err(&err));
        }
    }
}
