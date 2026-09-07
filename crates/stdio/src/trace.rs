//! Diagnostic trace events for the stdio transport (plans/0030).
//!
//! Field policy — metadata records only: frame/method names, session
//! and request ids, actual wire byte counts, routing classifications
//! and lifecycle stages. Never params, results, argv, environment
//! values, or remote error message text (classify those instead).
//! Provider stderr text is the one Full-only surface; see
//! `reader::spawn_stderr_reader`.

use crate::routing::{RequestId, RouteOutcome, SessionId};
use rootle_provider::ErrorKind;
use rootle_trace::EventKind;
use serde_json::{Map, Value, json};

/// Every record names the backend; the post-handshake display name is
/// deliberately not used (reader threads cannot see it, and one stable
/// label joins generations).
const PROVIDER: &str = "stdio";

/// Taxonomy label for error records — kinds only, never messages.
pub(crate) fn error_kind_label(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Auth => "auth",
        ErrorKind::RateLimited => "rate_limited",
        ErrorKind::NotFound => "not_found",
        ErrorKind::Network => "network",
        ErrorKind::Timeout => "timeout",
        ErrorKind::Provider => "provider",
        ErrorKind::Other => "other",
    }
}

/// Lazy `ProviderLifecycle` record: `fields` runs only while tracing
/// is enabled.
pub(crate) fn lifecycle<F>(stage: &str, fields: F)
where
    F: FnOnce(&mut Map<String, Value>),
{
    rootle_trace::record_with(EventKind::ProviderLifecycle, || {
        let mut map = Map::new();
        map.insert("provider".into(), json!(PROVIDER));
        map.insert("stage".into(), json!(stage));
        fields(&mut map);
        Value::Object(map)
    });
}

/// A request frame written to the child's stdin. `written` separates a
/// frame we attempted from one the pipe accepted; `bytes` counts the
/// full wire frame (payload + newline) either way.
pub(crate) fn tx_request(
    session: SessionId,
    id: RequestId,
    method: &str,
    bytes: u64,
    written: bool,
) {
    rootle_trace::record_with(EventKind::RpcMessage, || {
        json!({
            "provider": PROVIDER,
            "session": session.value(),
            "frame": "request",
            "dir": "tx",
            "id": id.wire(),
            "method": method,
            "bytes": bytes,
            "written": written,
        })
    });
}

/// The v1.1 advisory `$/cancelRequest` notification (best-effort).
pub(crate) fn tx_cancel(session: SessionId, id: u64, bytes: u64, written: bool) {
    rootle_trace::record_with(EventKind::RpcMessage, || {
        json!({
            "provider": PROVIDER,
            "session": session.value(),
            "frame": "cancel",
            "dir": "tx",
            "id": id,
            "method": "$/cancelRequest",
            "bytes": bytes,
            "written": written,
        })
    });
}

/// A final reply frame read from the child's stdout. `error_kind` is
/// the classified taxonomy label when the reply carries an error —
/// the remote message text itself is never recorded.
pub(crate) fn rx_response(
    session: SessionId,
    id: RequestId,
    bytes: u64,
    outcome: RouteOutcome,
    error_kind: Option<&'static str>,
) {
    rootle_trace::record_with(EventKind::RpcMessage, || {
        json!({
            "provider": PROVIDER,
            "session": session.value(),
            "frame": "response",
            "dir": "rx",
            "id": id.wire(),
            "bytes": bytes,
            "routed": outcome.label(),
            "error_kind": error_kind,
        })
    });
}

/// A `$/partial` streaming batch. `outcome` says whether it reached
/// its slot — a non-streaming request's partial is dropped, a retired
/// or unknown id is dropped, a stale reader generation is dropped.
pub(crate) fn rx_partial(session: SessionId, id: RequestId, bytes: u64, outcome: RouteOutcome) {
    rootle_trace::record_with(EventKind::RpcMessage, || {
        json!({
            "provider": PROVIDER,
            "session": session.value(),
            "frame": "partial",
            "dir": "rx",
            "id": id.wire(),
            "bytes": bytes,
            "routed": outcome.label(),
        })
    });
}

/// Any other notification frame from the provider (method name only).
pub(crate) fn rx_notification(session: SessionId, method: Option<&str>, bytes: u64) {
    rootle_trace::record_with(EventKind::RpcMessage, || {
        json!({
            "provider": PROVIDER,
            "session": session.value(),
            "frame": "notification",
            "dir": "rx",
            "method": method,
            "bytes": bytes,
        })
    });
}

/// An unroutable or unparseable line — byte count only, never text.
pub(crate) fn rx_malformed(session: SessionId, bytes: u64) {
    rootle_trace::record_with(EventKind::RpcMessage, || {
        json!({
            "provider": PROVIDER,
            "session": session.value(),
            "frame": "malformed",
            "dir": "rx",
            "bytes": bytes,
        })
    });
}

/// A request whose read deadline fired; its slot expires and any late
/// reply can only ever be classified `retired_id`.
pub(crate) fn request_expired(session: SessionId, id: RequestId, timeout_ms: u64) {
    rootle_trace::record_with(EventKind::RpcMessage, || {
        json!({
            "provider": PROVIDER,
            "session": session.value(),
            "frame": "request",
            "id": id.wire(),
            "outcome": "timeout",
            "timeout_ms": timeout_ms,
        })
    });
}
