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
        let Ok(mut message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if let Some(id) = message
            .get("id")
            .and_then(Value::as_u64)
            .and_then(RequestId::from_wire)
        {
            shared.routing.lock().response(session, id, message);
        } else if message.get("method").and_then(Value::as_str) == Some("$/partial")
            && let Some(id) = message
                .pointer("/params/id")
                .and_then(Value::as_u64)
                .and_then(RequestId::from_wire)
            && let Some(params) = message.get_mut("params").map(Value::take)
        {
            shared.routing.lock().partial(session, id, params);
        }
    }
    shared.routing.lock().disconnect(session);
    shared.changed.notify_all();
}
