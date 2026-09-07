//! Privacy-safe observations shared by app input, actions and workers.
//! Metadata retains identities and lengths; Full adds explicit user-visible text.
//! Serialization and payload construction happen only while capture is enabled.
mod actions;
mod events;
mod input;
mod state;

pub(crate) use actions::record_action;
pub(crate) use events::{
    event_name, record_event_accepted, record_event_received, record_event_rejected,
};
pub(crate) use input::{describe_key_with, record_key};
pub(crate) use state::record_state;

use crate::components::pane::EntryKind;
use rootle_provider::{ErrorKind, ProviderError};
use rootle_trace::EventKind;
use serde_json::{Value, json};

fn full() -> bool {
    rootle_trace::capture_content()
}

/// Text under the privacy contract: length in Metadata, exact in Full.
pub(crate) fn text_with(full: bool, value: &str) -> Value {
    if full {
        json!(value)
    } else {
        json!(value.len())
    }
}

pub(crate) fn text(value: &str) -> Value {
    text_with(full(), value)
}

fn opt_text_with(full: bool, value: &Option<String>) -> Value {
    value
        .as_deref()
        .map(|s| text_with(full, s))
        .unwrap_or(Value::Null)
}

fn error_kind(kind: ErrorKind) -> &'static str {
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

/// Remote errors are classified, never dumped. Full capture records messages
/// only through the state/frame surfaces that actually display them.
pub(crate) fn describe_error(error: &ProviderError) -> Value {
    json!({
        "kind": error_kind(error.kind),
        "message_bytes": error.message.len(),
        "retry_after_s": error.retry_after.map(|d| d.as_secs()),
    })
}

fn entry_kind(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Dir => "dir",
        EntryKind::File => "file",
        EntryKind::Repo => "repo",
        EntryKind::Org => "org",
    }
}

/// A spawn-time refusal: the job category was asked for but never
/// started (missing capability, offline mode, missing context).
/// `detail` is built lazily.
pub(crate) fn record_job_rejected(
    job: &'static str,
    reason: &'static str,
    detail: impl FnOnce() -> Value,
) {
    rootle_trace::record_with(EventKind::JobRejected, || {
        let mut fields = detail();
        fields["job"] = json!(job);
        fields["reason"] = json!(reason);
        fields
    });
}

#[cfg(test)]
mod tests;
