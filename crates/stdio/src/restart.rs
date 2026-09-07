//! One caller owns a rebuild attempt. Waiters observe its outcome without
//! paying for another attempt, and only a validated live reader is admitted.

use crate::StdioProvider;
use crate::process::spawn_process;
use crate::reader::spawn_reader;
use crate::routing::{Lifecycle, OutputState};
use rootle_provider::{ErrorKind, ProviderError, ProviderResult};
use serde_json::Value;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

pub(super) fn backoff_for(restart_attempt: u32) -> Duration {
    match restart_attempt {
        1 => Duration::from_secs(1),
        2 => Duration::from_secs(2),
        3 => Duration::from_secs(5),
        _ => Duration::from_secs(30),
    }
}

impl StdioProvider {
    pub(super) fn request(&self, method: &str, params: Value) -> ProviderResult<Value> {
        self.ensure_alive()?;
        self.exchange(method, params, true)
    }

    pub(super) fn ensure_alive(&self) -> ProviderResult<()> {
        let mut routing = self.shared.routing.lock();
        let mut waiting_for = None;
        loop {
            if waiting_for.is_some_and(|session| session != routing.session) {
                return Err(ProviderError::new(
                    ErrorKind::Provider,
                    "provider restart was superseded — try again",
                ));
            }
            match routing.lifecycle {
                Lifecycle::Alive => return Ok(()),
                Lifecycle::Respawning => {
                    waiting_for = Some(routing.session);
                    self.shared.changed.wait(&mut routing);
                }
                Lifecycle::Dead if waiting_for.is_some() => {
                    let reason = routing
                        .restart_error
                        .as_deref()
                        .unwrap_or("provider closed its output");
                    return Err(ProviderError::new(
                        ErrorKind::Provider,
                        format!("provider restart failed: {reason}"),
                    ));
                }
                Lifecycle::Dead => {
                    routing.begin_rebuild();
                    let attempt = routing.restarts.saturating_add(1);
                    drop(routing);
                    return self.rebuild(attempt);
                }
            }
        }
    }

    fn rebuild(&self, attempt: u32) -> ProviderResult<()> {
        let mut guard = RebuildGuard {
            provider: self,
            attempt,
            armed: true,
        };
        let result = self.rebuild_process(attempt);
        if result.is_err() {
            // A failed handshake may leave a live, silent child. Kill it
            // before joining its stdout reader, not on the next request.
            self.process.lock().terminate();
            if let Some(reader) = self.reader.lock().take() {
                let _ = reader.join();
            }
            if let Some(stderr_reader) = self.stderr_reader.lock().take() {
                let _ = stderr_reader.join();
            }
        }
        guard.armed = false;
        self.finish_rebuild(attempt, result.as_ref().map(|_| ()))
    }

    fn rebuild_process(&self, attempt: u32) -> ProviderResult<()> {
        let backoff = backoff_for(attempt);
        crate::trace::lifecycle("backoff", |fields| {
            fields.insert("attempt".into(), serde_json::json!(attempt));
            fields.insert(
                "backoff_ms".into(),
                serde_json::json!(backoff.as_millis() as u64),
            );
        });
        std::thread::sleep(backoff);
        if self.closed.load(Ordering::Acquire) {
            return Err(ProviderError::other("provider dropped during restart"));
        }
        // A write error can detect death before stdout EOF. Joining first
        // would block forever on a provider still holding its output pipe.
        self.process.lock().terminate();
        if let Some(reader) = self.reader.lock().take() {
            let _ = reader.join();
        }
        if let Some(stderr_reader) = self.stderr_reader.lock().take() {
            let _ = stderr_reader.join();
        }
        let environment: Vec<_> = self
            .env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        {
            let spawned = spawn_process(&self.command, &environment, self.stderr_mode);
            let mut current = self.process.lock();
            let mut routing = self.shared.routing.lock();
            let new_session = routing.session;
            match spawned {
                Ok((process, stdout, child_stderr)) => {
                    crate::trace::lifecycle("spawn", |fields| {
                        fields.insert("session".into(), serde_json::json!(new_session.value()));
                    });
                    *current = process;
                    routing.output = OutputState::Open;
                    let stderr_handle = child_stderr
                        .map(|stderr| crate::stderr::spawn(stderr, new_session))
                        .transpose()
                        .map_err(|error| {
                            rootle_trace::mark_incomplete("provider stderr reader could not start");
                            ProviderError::other(format!("start provider stderr reader: {error}"))
                        })?;
                    *self.stderr_reader.lock() = stderr_handle;
                    let reader = spawn_reader(stdout, Arc::clone(&self.shared), new_session)?;
                    *self.reader.lock() = Some(reader);
                    new_session
                }
                Err(error) => {
                    crate::trace::lifecycle("spawn_failed", |fields| {
                        fields.insert("session".into(), serde_json::json!(new_session.value()));
                    });
                    return Err(error);
                }
            }
        };
        self.handshake().map(|_| ())
    }

    fn finish_rebuild(
        &self,
        attempt: u32,
        outcome: Result<(), &ProviderError>,
    ) -> ProviderResult<()> {
        let mut routing = self.shared.routing.lock();
        let outcome = match outcome {
            Ok(()) if routing.output != OutputState::Open => {
                Err(ProviderError::other("provider closed after initialize"))
            }
            Ok(()) => Ok(()),
            Err(error) => Err(error.clone()),
        };
        match &outcome {
            Ok(()) => {
                routing.lifecycle = Lifecycle::Alive;
                routing.restarts = attempt;
                routing.restart_error = None;
            }
            Err(error) => {
                routing.lifecycle = Lifecycle::Dead;
                routing.restart_error = Some(error.message.clone());
            }
        }
        drop(routing);
        crate::trace::lifecycle("restart", |fields| {
            fields.insert("attempt".into(), serde_json::json!(attempt));
            fields.insert(
                "backoff_ms".into(),
                serde_json::json!(backoff_for(attempt).as_millis() as u64),
            );
            match &outcome {
                Ok(()) => {
                    fields.insert("outcome".into(), serde_json::json!("ok"));
                }
                // The kind only — restart error strings can carry the
                // provider program name (argv) or spawn errors.
                Err(error) => {
                    fields.insert("outcome".into(), serde_json::json!("failed"));
                    fields.insert(
                        "error_kind".into(),
                        serde_json::json!(crate::trace::error_kind_label(error.kind)),
                    );
                }
            }
        });
        self.shared.changed.notify_all();
        match &outcome {
            Ok(()) => {
                *self.failure_noticed.lock() = false;
                *self.notice.lock() = Some(format!(
                    "provider restarted (attempt {attempt}, backoff {}s)",
                    backoff_for(attempt).as_secs()
                ));
            }
            Err(error) => {
                let mut noticed = self.failure_noticed.lock();
                if !*noticed {
                    *noticed = true;
                    *self.notice.lock() = Some(format!(
                        "provider keeps failing to restart ({}) — running without it",
                        error.message
                    ));
                }
            }
        }
        outcome
    }
}

struct RebuildGuard<'a> {
    provider: &'a StdioProvider,
    attempt: u32,
    armed: bool,
}
impl Drop for RebuildGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let error = ProviderError::other("rebuild aborted unexpectedly");
            let _ = self.provider.finish_rebuild(self.attempt, Err(&error));
        }
    }
}

#[cfg(test)]
mod tests;
