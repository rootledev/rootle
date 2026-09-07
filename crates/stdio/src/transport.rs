//! Request registration/write and the caller-side inactivity wait. Routing
//! transitions live in `routing`; stdout parsing lives in `reader`.

use crate::StdioProvider;
use crate::routing::{Delivery, RequestId, SessionId};
use rootle_provider::{ErrorKind, ProviderError, ProviderResult};
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{atomic::Ordering, mpsc},
};

struct PendingReply {
    id: RequestId,
    session: SessionId,
    receiver: mpsc::Receiver<Delivery>,
}

impl StdioProvider {
    pub(super) fn exchange(
        &self,
        method: &str,
        params: Value,
        gated: bool,
    ) -> ProviderResult<Value> {
        let pending = self.send_request(method, params, gated, false)?;
        self.await_reply(pending, None)
    }

    pub(super) fn exchange_with_partials(
        &self,
        method: &str,
        params: Value,
        on_partial: &(dyn Fn(&Value) + Send + Sync),
    ) -> ProviderResult<Value> {
        self.ensure_alive()?;
        let pending = self.send_request(method, params, true, true)?;
        self.await_reply(pending, Some(on_partial))
    }

    fn send_request(
        &self,
        method: &str,
        params: Value,
        gated: bool,
        streaming: bool,
    ) -> ProviderResult<PendingReply> {
        // Lock order is process -> routing everywhere a write can race
        // replacement. Registration and stdin always belong to one epoch.
        let mut process = self.process.lock();
        let (id, receiver, session) = {
            let mut routing = self.shared.routing.lock();
            let (id, receiver) = routing.register(gated, streaming)?;
            (id, receiver, routing.session)
        };
        let line = json!({"jsonrpc":"2.0", "id": id.wire(), "method": method, "params": params})
            .to_string();
        // The wire frame is payload + newline; `written` separates an
        // attempted write from one the pipe accepted.
        let frame_bytes = line.len() as u64 + 1;
        if let Err(error) = writeln!(process.stdin, "{line}").and_then(|()| process.stdin.flush()) {
            crate::trace::tx_request(session, id, method, frame_bytes, false);
            let mut routing = self.shared.routing.lock();
            routing.expire(id);
            routing.disconnect(session);
            self.shared.changed.notify_all();
            return Err(ProviderError::new(
                ErrorKind::Provider,
                format!("provider write: {error}"),
            ));
        }
        crate::trace::tx_request(session, id, method, frame_bytes, true);
        Ok(PendingReply {
            id,
            session,
            receiver,
        })
    }

    fn await_reply(
        &self,
        pending: PendingReply,
        on_partial: Option<&(dyn Fn(&Value) + Send + Sync)>,
    ) -> ProviderResult<Value> {
        let PendingReply {
            id,
            session,
            receiver,
        } = pending;
        let _current = CurrentIdGuard::new(self, id);
        loop {
            match receiver.recv_timeout(self.timeout) {
                Ok(Delivery::Partial(params)) => {
                    // Only opted-in slots can receive this variant. A
                    // non-streaming call's deadline cannot be extended by
                    // unsolicited partial notifications.
                    if let Some(on_partial) = on_partial {
                        on_partial(&params);
                    }
                }
                Ok(Delivery::Response(mut message)) => {
                    if let Some(error) = message.get("error") {
                        return Err(error_from_reply(error));
                    }
                    return message
                        .get_mut("result")
                        .map(Value::take)
                        .ok_or_else(|| ProviderError::other("provider reply without result"));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.shared.routing.lock().expire(id);
                    crate::trace::request_expired(session, id, self.timeout.as_millis() as u64);
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
            }
        }
    }
}

struct CurrentIdGuard<'a> {
    provider: &'a StdioProvider,
    id: RequestId,
}
impl<'a> CurrentIdGuard<'a> {
    fn new(provider: &'a StdioProvider, id: RequestId) -> Self {
        provider.current_id.store(id.wire(), Ordering::Release);
        Self { provider, id }
    }
}
impl Drop for CurrentIdGuard<'_> {
    fn drop(&mut self) {
        let _ = self.provider.current_id.compare_exchange(
            self.id.wire(),
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

pub(super) fn cancel_notification(id: u64) -> String {
    json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":id}}).to_string()
}

pub(super) fn de<Response: serde::de::DeserializeOwned>(value: Value) -> ProviderResult<Response> {
    serde_json::from_value(value)
        .map_err(|error| ProviderError::other(format!("provider reply shape: {error}")))
}

fn error_kind_of(error: &Value) -> ErrorKind {
    match error
        .get("data")
        .and_then(|data| data.get("kind"))
        .and_then(Value::as_str)
    {
        Some("auth") => ErrorKind::Auth,
        Some("rate_limited") => ErrorKind::RateLimited,
        Some("not_found") => ErrorKind::NotFound,
        Some("network") => ErrorKind::Network,
        Some("timeout") => ErrorKind::Timeout,
        Some("provider") => ErrorKind::Provider,
        _ => ErrorKind::Other,
    }
}

/// The classified taxonomy label for a reply carrying an error —
/// diagnostics record the class, never the remote message text.
pub(super) fn classify_reply_error(message: &Value) -> Option<&'static str> {
    let error = message.get("error")?;
    Some(match error_kind_of(error) {
        ErrorKind::Auth => "auth",
        ErrorKind::RateLimited => "rate_limited",
        ErrorKind::NotFound => "not_found",
        ErrorKind::Network => "network",
        ErrorKind::Timeout => "timeout",
        ErrorKind::Provider => "provider",
        _ => "other",
    })
}

fn error_from_reply(error: &Value) -> ProviderError {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("provider error");
    let data = error.get("data");
    let error = ProviderError::new(error_kind_of(error), message);
    match data
        .and_then(|data| data.get("retry_after_s"))
        .and_then(Value::as_u64)
    {
        Some(seconds) => error.with_retry_after(std::time::Duration::from_secs(seconds)),
        None => error,
    }
}
