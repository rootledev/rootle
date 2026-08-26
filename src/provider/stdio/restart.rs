//! The recovery machine (plans/0008 §5): EOF marks the transport
//! dead; the next request respawns the child with bounded backoff,
//! re-runs the initialize handshake, and only then proceeds. A
//! successful restart leaves a notice (`take_notice`) for the status
//! line.

use super::StdioProvider;
use super::process::spawn_process;
use super::transport::{Lifecycle, reader_loop};
use crate::provider::{ErrorKind, ProviderError, ProviderResult};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Bounded respawn backoff (plans/0008 §5): 1s → 2s → 5s → 30s cap.
pub(super) fn backoff_for(restart_attempt: u32) -> Duration {
    match restart_attempt {
        1 => Duration::from_secs(1),
        2 => Duration::from_secs(2),
        3 => Duration::from_secs(5),
        _ => Duration::from_secs(30),
    }
}

impl StdioProvider {
    /// One NDJSON-RPC round trip with liveness: rebuild first if the
    /// reader thread reported EOF (plans/0008 §5).
    pub(super) fn request(&self, method: &str, params: Value) -> ProviderResult<Value> {
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

#[cfg(test)]
mod tests;
