//! Stdio provider (plans/0005): an external child process speaking
//! newline-delimited JSON-RPC 2.0 on stdin/stdout — the LSP model.
//! Wrap any internal source-control system with a small script
//! (see examples/providers/) and point `[provider] command` at it.
//!
//! Layout: this file is the surface — state + spawn constructors.
//! `process.rs` spawns the child; `transport.rs` owns the reader
//! thread + reply routing; `handshake.rs` runs the initialize round
//! trip; `restart.rs` is the respawn-with-backoff recovery machine;
//! `wire.rs` maps `trait Provider` methods onto round trips;
//! `trace.rs` emits the diagnostic records (plans/0030).
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

use self::process::{Process, StderrMode, spawn_process};
use self::reader::spawn_reader;
use self::routing::Shared;
use parking_lot::Mutex;
use rootle_provider::{Capabilities, ProviderResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

mod handshake;
mod process;
mod reader;
mod restart;
mod routing;
mod stderr;
mod trace;
mod transport;
mod wire;

#[cfg(test)]
mod tests;

/// Default per-request read deadline; `[provider] timeout_ms`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(30_000);

pub struct StdioProvider {
    name: String,
    icon: Option<String>,
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
    /// Bounded stderr drainer for the same generation — present only
    /// under Full-content tracing (Null mode); joined wherever the
    /// stdout reader is, always after the child is terminated.
    stderr_reader: Mutex<Option<stderr::StderrReader>>,
    reader: Mutex<Option<JoinHandle<()>>>,
    /// One-shot UI notice (a successful restart, a config warning),
    /// drained via `take_notice` (plans/0008 §5).
    notice: Mutex<Option<String>>,
    /// 0022 M1: a rebuild-failure streak notices once per streak
    /// (reset on recovery) — not once per attempt.
    failure_noticed: Mutex<bool>,
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
        process.terminate();
        if let Some(handle) = self.reader.lock().take() {
            let _ = handle.join();
        }
        // Stop the bounded drainer even if descendants inherited the pipe.
        if let Some(handle) = self.stderr_reader.lock().take() {
            let _ = handle.join();
        }
    }
}

#[derive(Default)]
struct SpawnCache {
    bytes: u64,
    directory: Option<std::path::PathBuf>,
}

impl StdioProvider {
    /// Spawn the provider and run the initialize handshake.
    pub fn spawn(command: &[String], timeout: Duration) -> ProviderResult<Self> {
        Self::spawn_inner(
            command,
            timeout,
            &[],
            StderrMode::Null,
            SpawnCache::default(),
        )
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
        Self::spawn_inner(
            command,
            timeout,
            &[],
            mode,
            SpawnCache {
                bytes: cache_bytes,
                directory: cache_dir,
            },
        )
    }

    /// Spawn with extra environment for the child process — the
    /// test-binary re-exec seam (fakes pick their script by env).
    pub fn spawn_with_env(
        command: &[String],
        timeout: Duration,
        env: &[(&str, &str)],
    ) -> ProviderResult<Self> {
        Self::spawn_inner(
            command,
            timeout,
            env,
            StderrMode::Null,
            SpawnCache::default(),
        )
    }

    fn spawn_inner(
        command: &[String],
        timeout: Duration,
        env: &[(&str, &str)],
        stderr_mode: StderrMode,
        cache: SpawnCache,
    ) -> ProviderResult<Self> {
        let spawned = spawn_process(command, env, stderr_mode);
        let shared = Arc::new(Shared::default());
        let session = shared.routing.lock().session;
        let (mut process, stdout, child_stderr) = match spawned {
            Ok(spawned) => {
                self::trace::lifecycle("spawn", |fields| {
                    fields.insert("session".into(), serde_json::json!(session.value()));
                });
                spawned
            }
            Err(error) => {
                self::trace::lifecycle("spawn_failed", |fields| {
                    fields.insert("session".into(), serde_json::json!(session.value()));
                });
                return Err(error);
            }
        };
        let reader = spawn_reader(stdout, Arc::clone(&shared), session)?;
        let stderr_handle = match child_stderr
            .map(|stderr| self::stderr::spawn(stderr, session))
            .transpose()
        {
            Ok(handle) => handle,
            Err(error) => {
                rootle_trace::mark_incomplete("provider stderr reader could not start");
                process.terminate();
                let _ = reader.join();
                return Err(rootle_provider::ProviderError::other(format!(
                    "start provider stderr reader: {error}"
                )));
            }
        };
        let provider = StdioProvider {
            name: "stdio".into(),
            icon: None,
            capabilities: Capabilities {
                commit: false,
                orgs: true,
                code_search: true,
                file_search: true,
                // v1.5 defaults false until the handshake says
                // otherwise (default-branch-only providers).
                refs: false,
                log: false,
                blame: false,
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
            stderr_reader: Mutex::new(stderr_handle),
            reader: Mutex::new(Some(reader)),
            notice: Mutex::new(None),
            failure_noticed: Mutex::new(false),
            closed: AtomicBool::new(false),
            cache_bytes: cache.bytes,
            cache_dir: cache.directory,
            cache_used: parking_lot::Mutex::new(None),
        };
        // Same deadline as any request: a provider that hangs on
        // startup fails into the github fallback instead of blocking
        // launch (plans/0008 §1).
        let reply = provider.handshake()?;
        provider.initialize_from(reply)
    }
}
