//! rootle's opt-in diagnostic session sink: one private, bounded JSONL file
//! per session, written by a dedicated thread.
//!
//! Producers never perform file I/O, never wait for the writer, and never
//! wait for queue capacity — a full queue, an oversize record, a cap or a
//! writer failure is an explicit capture failure surfaced through
//! [`take_failure`] and [`TraceSession::finish`], never a silent drop.
//! Only the writer thread touches the file, flushing every batch it
//! receives, and every file it closes ends with a terminal `trace_end`
//! marker stating whether the capture is complete and why. A capped,
//! failed, panicked or abandoned capture can never look complete, and a
//! crash that prevents the marker leaves its absence as the verdict.
//!
//! Envelopes carry `schema_version`, the writer-owned `seq`, `elapsed_us`,
//! a `thread` label, the thread's optional `operation_id` correlation, the
//! `event` kind and producer `fields`; sequence and timestamps agree with
//! admission order under concurrent producers.
//!
//! Metadata capture is diagnostic, not anonymous: paths, repository
//! identities, revisions and timing can still be sensitive — inspect files
//! before sharing. Nothing here ever prints to stdout/stderr; the only
//! outputs are the file and explicit `Result`s.

mod bounded;
mod context;
mod event;
mod writer;

pub use context::{OperationId, in_operation, operation_id};
pub use event::{
    ContentPolicy, EventKind, Limits, MAX_CAPTURE_BYTES, MAX_CAPTURE_EVENTS, MAX_RECORD_BYTES,
    SCHEMA_VERSION, TERMINAL_RESERVE, TraceOptions,
};

use arc_swap::ArcSwapOption;
use parking_lot::{Condvar, Mutex, MutexGuard};
use serde::Serialize;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, LazyLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A full queue is a visible capture failure, never silent loss; the
/// writer drains whole batches, so this only trips on writer stalls.
pub(crate) const QUEUE_CAPACITY: usize = 64;
/// Panic durability and shutdown both wait at most this long for the
/// writer, so a stalled disk cannot hang unwinding or exit either.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// Panic text is capped well below the per-record limit so admitting the
/// panic record can itself never trip the cap.
const PANIC_MESSAGE_CHARS: usize = 2048;
const PANIC_BACKTRACE_CHARS: usize = 8192;

static ACTIVE: LazyLock<ArcSwapOption<Recorder>> = LazyLock::new(ArcSwapOption::empty);
/// Only startup/close mutate the slot; producers and panic hooks take atomic
/// snapshots, never this lock.
static LIFECYCLE: Mutex<()> = Mutex::new(());
/// Fast disabled path: even the atomic Arc slot stays untouched without a session.
static SESSION_PRESENT: AtomicBool = AtomicBool::new(false);

/// How the session ended. Only an explicit `finish` may ever let the
/// terminal marker claim completeness.
mod close {
    pub(crate) const FINISH: u8 = 1;
    pub(crate) const ABANDON: u8 = 2;
}

#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("a trace session is already active")]
    AlreadyActive,
    #[error("cannot create trace {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot start trace writer: {0}")]
    Spawn(std::io::Error),
    #[error("incomplete trace: {0}")]
    Incomplete(String),
}

/// First failure wins; later failures cannot overwrite the cause.
#[derive(Default)]
pub(crate) struct Failure {
    message: Mutex<Option<String>>,
    reported: AtomicBool,
    failed: AtomicBool,
}

impl Failure {
    pub(crate) fn set(&self, message: impl FnOnce() -> String) {
        let mut failure = self.message.lock();
        if failure.is_none() {
            *failure = Some(message());
        }
        self.failed.store(true, Ordering::Release);
    }
    pub(crate) fn present(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

#[derive(Default)]
pub(crate) struct SignalState {
    pub(crate) panic_flushed: bool,
    pub(crate) done: bool,
}

/// Bounded writer signaling: panic admission waits here for durability and
/// shutdown waits here instead of an unjoinable stall.
#[derive(Default)]
pub(crate) struct Signal {
    state: Mutex<SignalState>,
    ready: Condvar,
}

impl Signal {
    pub(crate) fn mark_panic_flushed(&self) {
        self.state.lock().panic_flushed = true;
        self.ready.notify_all();
    }
    pub(crate) fn mark_done(&self) {
        self.state.lock().done = true;
        self.ready.notify_all();
    }
    /// Wait until `satisfied` holds or `limit` elapses; false on timeout.
    pub(crate) fn wait_for(
        &self,
        satisfied: impl Fn(&SignalState) -> bool,
        limit: Duration,
    ) -> bool {
        let mut state = self.state.lock();
        let deadline = Instant::now() + limit;
        while !satisfied(&state) {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            if remaining.is_zero() {
                return false;
            }
            if self.ready.wait_for(&mut state, remaining).timed_out() && !satisfied(&state) {
                return false;
            }
        }
        true
    }
}

/// State shared with the writer thread.
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) failure: Failure,
    pub(crate) close_kind: AtomicU8,
    pub(crate) signal: Signal,
    pub(crate) panicked: AtomicBool,
}

impl Shared {
    pub(crate) fn close_kind(&self) -> u8 {
        self.close_kind.load(Ordering::Relaxed)
    }
    /// Whether ordinary records must stop being admitted. The panic record
    /// itself is exempt: it is the session's last word.
    fn capture_dead(&self) -> bool {
        self.failure.present() || self.panicked.load(Ordering::Acquire) || self.close_kind() != 0
    }
}

pub(crate) struct Record {
    pub(crate) kind: EventKind,
    pub(crate) elapsed_us: u128,
    pub(crate) thread: Arc<str>,
    pub(crate) operation: Option<OperationId>,
    pub(crate) fields: Vec<u8>,
}

pub(crate) struct Recorder {
    pub(crate) sender: Mutex<Option<SyncSender<Record>>>,
    pub(crate) shared: Arc<Shared>,
    started: Instant,
    max_record: usize,
    content: ContentPolicy,
}

/// The owning lifetime of a trace. [`TraceSession::finish`] is the
/// explicit reporting boundary; `Drop` still finalizes the file durably,
/// but as an abandoned capture, never a complete one.
pub struct TraceSession {
    recorder: Arc<Recorder>,
    worker: Option<JoinHandle<()>>,
}

/// Start the process's single trace session, creating `path` exclusively.
///
/// The file is new, private (0600 on Unix) and never appended to;
/// existing files and symlinks are refused with [`TraceError::Open`], as
/// are missing parent directories. A second concurrent session is refused
/// with [`TraceError::AlreadyActive`], and limits outside the crate's
/// hard bounds with [`TraceError::Incomplete`].
pub fn start(path: &Path, options: TraceOptions) -> Result<TraceSession, TraceError> {
    if !options.limits.valid() {
        return Err(TraceError::Incomplete("invalid capture limits".into()));
    }
    let _lifecycle = LIFECYCLE.lock();
    if ACTIVE.load().is_some() {
        return Err(TraceError::AlreadyActive);
    }
    let mut open = OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    let file = open.open(path).map_err(|source| TraceError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let (sender, receiver) = sync_channel(QUEUE_CAPACITY);
    let shared = Arc::new(Shared::default());
    let writer_shared = Arc::clone(&shared);
    let limits = options.limits;
    let worker = std::thread::Builder::new()
        .name("rootle-trace".into())
        .spawn(move || writer::run(file, receiver, writer_shared, limits))
        .map_err(TraceError::Spawn)?;
    let recorder = Arc::new(Recorder {
        sender: Mutex::new(Some(sender)),
        shared,
        started: Instant::now(),
        max_record: limits.max_record_bytes,
        content: options.content,
    });
    ACTIVE.store(Some(Arc::clone(&recorder)));
    SESSION_PRESENT.store(true, Ordering::Release);
    Ok(TraceSession {
        recorder,
        worker: Some(worker),
    })
}

#[inline]
pub fn enabled() -> bool {
    SESSION_PRESENT.load(Ordering::Relaxed)
        && ACTIVE
            .load()
            .as_ref()
            .is_some_and(|recorder| !recorder.shared.capture_dead())
}

/// Whether [`ContentPolicy::Full`] was selected: producers gate sensitive
/// input/UI text and provider stderr on this.
#[inline]
pub fn capture_content() -> bool {
    SESSION_PRESENT.load(Ordering::Relaxed)
        && ACTIVE.load().as_ref().is_some_and(|recorder| {
            !recorder.shared.capture_dead() && recorder.content == ContentPolicy::Full
        })
}

/// Lazy producer: no payload construction or allocation when disabled.
pub fn record_with<T: Serialize>(kind: EventKind, fields: impl FnOnce() -> T) {
    let Some(recorder) = recording_snapshot() else {
        return;
    };
    // Pin before invoking user serialization/payload construction. A finish or
    // later start cannot redirect an in-flight Full payload into another file.
    let fields = fields();
    record_to(&recorder, kind, &fields);
}

pub fn record<T: Serialize>(kind: EventKind, fields: &T) {
    let Some(recorder) = recording_snapshot() else {
        return;
    };
    record_to(&recorder, kind, fields);
}

fn recording_snapshot() -> Option<Arc<Recorder>> {
    if !SESSION_PRESENT.load(Ordering::Relaxed) {
        return None;
    }
    let recorder = ACTIVE.load_full()?;
    (!recorder.shared.capture_dead()).then_some(recorder)
}

fn record_to<T: Serialize>(recorder: &Recorder, kind: EventKind, fields: &T) {
    let mut sender = recorder.sender.lock();
    admit(
        recorder,
        &mut sender,
        kind,
        |out| serde_json::to_writer(out, fields),
        false,
    );
}

/// Stamp, cap and admit one record under the caller's admission lock.
/// Serializing here bounds simultaneous encodings and keeps `elapsed_us`
/// nondecreasing in the writer's receive order; the lock never covers
/// file I/O or queue waits — `try_send` is nonblocking by construction.
fn admit(
    recorder: &Recorder,
    sender: &mut MutexGuard<'_, Option<SyncSender<Record>>>,
    kind: EventKind,
    encode: impl FnOnce(&mut bounded::Bytes) -> serde_json::Result<()>,
    urgent: bool,
) -> bool {
    if sender.is_none() {
        return false;
    }
    if !urgent && recorder.shared.capture_dead() {
        sender.take();
        return false;
    }
    let mut bytes = bounded::Bytes::new(recorder.max_record);
    if encode(&mut bytes).is_err() {
        recorder
            .shared
            .failure
            .set(|| "record exceeds cap or cannot serialize".into());
        sender.take();
        return false;
    }
    let record = Record {
        kind,
        elapsed_us: recorder.started.elapsed().as_micros(),
        thread: context::thread_label(),
        operation: context::current_operation(),
        fields: bytes.into_vec(),
    };
    if sender
        .as_ref()
        .expect("admission checked the sender")
        .try_send(record)
        .is_err()
    {
        recorder
            .shared
            .failure
            .set(|| "capture queue full or writer unavailable".into());
        sender.take();
        return false;
    }
    true
}

/// Best-effort panic capture, safe to call from a panic hook.
///
/// Marks the capture incomplete before touching any lock, then admits one
/// `panic` record without ever blocking: if a producer died holding the
/// admission lock the record is skipped, not waited for. Raw message and
/// backtrace text are included only under [`ContentPolicy::Full`] — even
/// when the caller supplies them — and are capped so admitting this record
/// can never trip the record cap. On successful admission the writer gets
/// a bounded window to flush it to disk.
pub fn panic_record(
    location: Option<(&str, u32, u32)>,
    message: Option<&str>,
    backtrace: Option<&str>,
) {
    if !SESSION_PRESENT.load(Ordering::Relaxed) {
        return;
    }
    let Some(recorder) = ACTIVE.load_full() else {
        return;
    };
    // Both policy and panic state belong to this exact session. Atomic slot
    // reads remain available when a producer/start/close holds another lock.
    recorder.shared.panicked.store(true, Ordering::Release);
    let full = recorder.content == ContentPolicy::Full;
    let message = full
        .then_some(message)
        .flatten()
        .map(|text| cap_text(text, PANIC_MESSAGE_CHARS));
    let backtrace = full
        .then_some(backtrace)
        .flatten()
        .map(|text| cap_text(text, PANIC_BACKTRACE_CHARS));
    let fields = serde_json::json!({
        "location": location.map(|(file, line, column)| serde_json::json!({
            "file": file,
            "line": line,
            "column": column,
        })),
        "message": message,
        "backtrace": backtrace,
    });
    let admitted = match recorder.sender.try_lock() {
        Some(mut sender) => admit(
            &recorder,
            &mut sender,
            EventKind::Panic,
            |out| serde_json::to_writer(out, &fields),
            true,
        ),
        // A producer died holding the admission lock; never block unwinding.
        None => false,
    };
    if admitted {
        recorder
            .shared
            .signal
            .wait_for(|state| state.panic_flushed || state.done, SHUTDOWN_TIMEOUT);
    }
}

/// Report a capture failure once (for a status line); [`TraceSession::finish`]
/// still returns it as an error.
pub fn take_failure() -> Option<String> {
    if !SESSION_PRESENT.load(Ordering::Relaxed) {
        return None;
    }
    let recorder = ACTIVE.load_full()?;
    if recorder.shared.failure.reported.load(Ordering::Relaxed) {
        return None;
    }
    let message = if recorder.shared.panicked.load(Ordering::Acquire) {
        "capture interrupted by panic".to_owned()
    } else {
        recorder.shared.failure.message.lock().clone()?
    };
    (!recorder
        .shared
        .failure
        .reported
        .swap(true, Ordering::Relaxed))
    .then_some(message)
}

/// A producer could not observe an explicitly requested surface. Preserve the
/// failure verdict and stop admission rather than writing a complete-looking log.
pub fn mark_incomplete(reason: &'static str) {
    let Some(recorder) = recording_snapshot() else {
        return;
    };
    record_to(
        &recorder,
        EventKind::Error,
        &serde_json::json!({"scope":"capture","reason":reason}),
    );
    recorder.shared.failure.set(|| reason.to_owned());
    recorder.sender.lock().take();
}

fn cap_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut capped: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        capped.push('…');
    }
    capped
}

impl TraceSession {
    /// End the capture explicitly: drain the writer (bounded), then report
    /// the first failure, cap or panic, if any. The only path that can
    /// leave a file marked complete.
    pub fn finish(mut self) -> Result<(), TraceError> {
        self.close(true)
    }

    fn close(&mut self, explicit: bool) -> Result<(), TraceError> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let shared = &self.recorder.shared;
        {
            let _lifecycle = LIFECYCLE.lock();
            shared.close_kind.store(
                if explicit {
                    close::FINISH
                } else {
                    close::ABANDON
                },
                Ordering::Release,
            );
            // Close admission before another session may become visible.
            self.recorder.sender.lock().take();
            if ACTIVE
                .load()
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, &self.recorder))
            {
                SESSION_PRESENT.store(false, Ordering::Release);
                ACTIVE.store(None);
            }
        }
        // Bounded shutdown: a stalled writer (blocked disk, dead mount) can
        // hang a join forever, so wait on its completion signal instead.
        // The detached thread is harmless — without the marker the file
        // already reads as incomplete.
        if !shared.signal.wait_for(|state| state.done, SHUTDOWN_TIMEOUT) {
            shared.failure.set(|| {
                format!(
                    "trace writer did not finish within {}s",
                    SHUTDOWN_TIMEOUT.as_secs()
                )
            });
        } else if worker.join().is_err() {
            shared.failure.set(|| "trace writer panicked".into());
        }
        // The writer may have observed a panic after shutdown started. Report
        // that same verdict rather than returning Ok for an incomplete file.
        if shared.panicked.load(Ordering::Acquire) {
            shared.failure.set(|| "capture interrupted by panic".into());
        }
        match shared.failure.message.lock().clone() {
            Some(error) => Err(TraceError::Incomplete(error)),
            None => Ok(()),
        }
    }
}

impl Drop for TraceSession {
    fn drop(&mut self) {
        // Explicit finish is the reporting boundary. Drop still finalizes
        // durably during unwinding or a forgotten session — but as an
        // abandoned capture, so it can never pass for a clean one.
        let _ = self.close(false);
    }
}

#[cfg(test)]
mod tests;
