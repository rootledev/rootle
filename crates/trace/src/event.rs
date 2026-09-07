//! Schema vocabulary shared by every producer; payload `fields` keep their
//! domain's types and stay under the per-record cap.
//!
//! Schema 1 is rootle's initial closed vocabulary plus the always-present
//! terminal `TraceEnd` marker that distinguishes a complete capture from a
//! capped, failed, panicked or abandoned one.

use serde::{Deserialize, Serialize};

/// Envelope schema version for every rootle session trace.
pub const SCHEMA_VERSION: u32 = 1;
/// Hard upper bounds a capture may use. They exist so a runaway producer
/// cannot fill the disk; `start` refuses anything outside them.
pub const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_CAPTURE_EVENTS: u64 = 100_000;
pub const MAX_RECORD_BYTES: usize = 256 * 1024;
/// The writer always reserves this much of the byte budget for the
/// terminal marker, so a capture that hits its cap still ends legibly.
pub const TERMINAL_RESERVE: usize = 1024;

/// The closed event vocabulary. Payloads are producer-defined `fields`;
/// the kind only fixes the category so files stay joinable across crates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    SessionStart,
    SessionEnd,
    TraceEnd,
    Input,
    Action,
    State,
    Render,
    Resize,
    JobStarted,
    JobFinished,
    JobRejected,
    RpcMessage,
    ProviderLifecycle,
    ProviderStderr,
    HttpRequest,
    HttpResponse,
    Cache,
    Config,
    ExternalCommand,
    Error,
    Panic,
}

/// What producer payloads may carry.
///
/// `Metadata` (the default) records kinds, lengths, identities and timing —
/// diagnostic, not anonymous: paths, repository identities, revisions and
/// operation names can still be sensitive. `Full` is an explicit opt into
/// sensitive capture (typed text, UI field text, provider stderr); it adds
/// surfaces, it is not a redaction boundary. Authorization headers,
/// environment values, command arguments and raw RPC/HTTP bodies are never
/// recorded automatically under either policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ContentPolicy {
    /// Structure, lengths and identities only.
    #[default]
    Metadata,
    /// Explicit sensitive capture: exact input/UI text and provider stderr.
    Full,
}

/// Bounded capture: total bytes, total events, per-record bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_events: u64,
    pub max_record_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_CAPTURE_BYTES,
            max_events: MAX_CAPTURE_EVENTS,
            max_record_bytes: MAX_RECORD_BYTES,
        }
    }
}

impl Limits {
    /// Hard bounds only — nothing, including callers, may lift a capture
    /// past the crate maxima, and the writer needs room for its terminal
    /// marker.
    pub fn valid(&self) -> bool {
        self.max_bytes >= TERMINAL_RESERVE * 2
            && self.max_bytes <= MAX_CAPTURE_BYTES
            && self.max_events > 0
            && self.max_events <= MAX_CAPTURE_EVENTS
            && self.max_record_bytes > 0
            && self.max_record_bytes <= MAX_RECORD_BYTES
    }
}

/// Startup controls for [`crate::start`].
#[derive(Debug, Default, Clone, Copy)]
pub struct TraceOptions {
    pub content: ContentPolicy,
    pub limits: Limits,
}
