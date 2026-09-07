//! Request registration/write and the caller-side inactivity wait. Routing
//! transitions live in `routing`; stdout parsing lives in `reader`.

use crate::StdioProvider;
use crate::routing::{Delivery, RequestId};
use rootle_provider::{ErrorKind, ProviderError, ProviderResult};
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{atomic::Ordering, mpsc},
};

impl StdioProvider {
    pub(super) fn exchange(
        &self,
        method: &str,
        params: Value,
        gated: bool,
    ) -> ProviderResult<Value> {
        let (id, receiver) = self.send_request(method, params, gated, false)?;
        self.await_reply(id, receiver, None)
    }

    pub(super) fn exchange_with_partials(
        &self,
        method: &str,
        params: Value,
        on_partial: &(dyn Fn(&Value) + Send + Sync),
    ) -> ProviderResult<Value> {
        self.ensure_alive()?;
        let (id, receiver) = self.send_request(method, params, true, true)?;
        self.await_reply(id, receiver, Some(on_partial))
    }

    fn send_request(
        &self,
        method: &str,
        params: Value,
        gated: bool,
        streaming: bool,
    ) -> ProviderResult<(RequestId, mpsc::Receiver<Delivery>)> {
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
        if let Err(error) = writeln!(process.stdin, "{line}").and_then(|()| process.stdin.flush()) {
            let mut routing = self.shared.routing.lock();
            routing.expire(id);
            routing.disconnect(session);
            self.shared.changed.notify_all();
            return Err(ProviderError::new(
                ErrorKind::Provider,
                format!("provider write: {error}"),
            ));
        }
        Ok((id, receiver))
    }

    fn await_reply(
        &self,
        id: RequestId,
        receiver: mpsc::Receiver<Delivery>,
        on_partial: Option<&(dyn Fn(&Value) + Send + Sync)>,
    ) -> ProviderResult<Value> {
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
                Ok(Delivery::Response(message)) => {
                    if let Some(error) = message.get("error") {
                        return Err(error_from_reply(error));
                    }
                    return message
                        .get("result")
                        .cloned()
                        .ok_or_else(|| ProviderError::other("provider reply without result"));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.shared.routing.lock().expire(id);
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

fn error_from_reply(error: &Value) -> ProviderError {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("provider error");
    let data = error.get("data");
    let kind = match data
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
    };
    let error = ProviderError::new(kind, message);
    match data
        .and_then(|data| data.get("retry_after_s"))
        .and_then(Value::as_u64)
    {
        Some(seconds) => error.with_retry_after(std::time::Duration::from_secs(seconds)),
        None => error,
    }
}
