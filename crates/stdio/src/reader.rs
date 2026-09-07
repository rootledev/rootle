//! One stdout pump per process generation. No old reader may mutate the
//! routing slots or lifecycle of its replacement.

use crate::routing::{RequestId, SessionId, Shared};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    process::ChildStdout,
    sync::Arc,
};

pub(crate) fn spawn_reader(
    stdout: ChildStdout,
    shared: Arc<Shared>,
    session: SessionId,
) -> rootle_provider::ProviderResult<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("rootle-provider-reader".into())
        .spawn(move || reader_loop(stdout, shared, session))
        .map_err(|error| {
            rootle_provider::ProviderError::other(format!("start provider reader: {error}"))
        })
}

pub(crate) fn reader_loop(stdout: ChildStdout, shared: Arc<Shared>, session: SessionId) {
    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match stdout.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // The frame's wire length is what was read (newline included);
        // text itself is never recorded.
        let frame_bytes = line.len() as u64;
        let Ok(mut message) = serde_json::from_str::<Value>(line.trim()) else {
            crate::trace::rx_malformed(session, frame_bytes);
            continue;
        };
        if let Some(id) = message
            .get("id")
            .and_then(Value::as_u64)
            .and_then(RequestId::from_wire)
        {
            // Classified before the reply moves into its slot: the
            // trace names the error kind, never the remote message.
            let error_kind = crate::transport::classify_reply_error(&message);
            let outcome = shared.routing.lock().response(session, id, message);
            crate::trace::rx_response(session, id, frame_bytes, outcome, error_kind);
        } else if message.get("id").is_some() {
            // Response-shaped but the id is unusable — it can never be
            // routed; the byte count is all that is honestly recordable.
            crate::trace::rx_malformed(session, frame_bytes);
        } else if message.get("method").and_then(Value::as_str) == Some("$/partial")
            && let Some(id) = message
                .pointer("/params/id")
                .and_then(Value::as_u64)
                .and_then(RequestId::from_wire)
            && let Some(params) = message.get_mut("params").map(Value::take)
        {
            let outcome = shared.routing.lock().partial(session, id, params);
            crate::trace::rx_partial(session, id, frame_bytes, outcome);
        } else {
            let method = message.get("method").and_then(Value::as_str);
            crate::trace::rx_notification(session, method, frame_bytes);
        }
    }
    let dropped = shared.routing.lock().disconnect(session);
    crate::trace::lifecycle("eof", |fields| {
        fields.insert("session".into(), serde_json::json!(session.value()));
        fields.insert("requests_dropped".into(), serde_json::json!(dropped));
    });
    shared.changed.notify_all();
}
