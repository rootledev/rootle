//! Only this worker writes to disk. It flushes each available batch, keeps
//! the capture inside its bounds, and ends writable files with an explicit
//! terminal marker. A write failure can prevent the marker; its absence means
//! incomplete rather than falsely promising a complete trace.

use crate::{
    EventKind, Limits, QUEUE_CAPACITY, Record, SCHEMA_VERSION, Shared, TERMINAL_RESERVE, close,
};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

/// The writer's own envelope label; it produces the terminal marker.
const WRITER_THREAD: &str = "rootle-trace-writer";
pub(crate) fn run(file: File, receiver: Receiver<Record>, shared: Arc<Shared>, limits: Limits) {
    run_sink(BufWriter::new(file), receiver, shared, limits)
}

fn run_sink(sink: impl Write, receiver: Receiver<Record>, shared: Arc<Shared>, limits: Limits) {
    if let Err(error) = drain(sink, receiver, &shared, limits) {
        shared
            .failure
            .set(move || format!("writer I/O failed: {error}"));
    }
    shared.signal.mark_done();
}

fn line(out: &mut impl Write, seq: u64, record: &Record) -> io::Result<()> {
    write!(
        out,
        "{{\"schema_version\":{SCHEMA_VERSION},\"seq\":{seq},\"elapsed_us\":{},\"thread\":",
        record.elapsed_us
    )?;
    serde_json::to_writer(&mut *out, record.thread.as_ref())?;
    if let Some(operation) = record.operation {
        write!(out, ",\"operation_id\":{}", operation.0)?;
    }
    out.write_all(b",\"event\":")?;
    serde_json::to_writer(&mut *out, &record.kind)?;
    out.write_all(b",\"fields\":")?;
    out.write_all(&record.fields)?;
    out.write_all(b"}\n")
}

fn drain(
    mut out: impl Write,
    receiver: Receiver<Record>,
    shared: &Shared,
    limits: Limits,
) -> io::Result<()> {
    let (mut seq, mut bytes, mut elapsed) = (0u64, 0usize, 0u128);
    let mut capped = false;
    'admission: while let Ok(first) = receiver.recv() {
        let mut batch = std::iter::once(first).chain(receiver.try_iter().take(QUEUE_CAPACITY - 1));
        let mut panic_in_batch = false;
        for record in &mut batch {
            let mut encoded = Vec::with_capacity(record.fields.len() + 160);
            line(&mut encoded, seq + 1, &record)?;
            if seq >= limits.max_events
                || encoded.len()
                    > limits
                        .max_bytes
                        .saturating_sub(TERMINAL_RESERVE)
                        .saturating_sub(bytes)
            {
                capped = true;
                shared.failure.set(|| "capture limit reached".into());
                break 'admission;
            }
            out.write_all(&encoded)?;
            bytes += encoded.len();
            seq += 1;
            elapsed = record.elapsed_us;
            panic_in_batch |= record.kind == EventKind::Panic;
        }
        out.flush()?;
        // A panic is durable only after its own record was written, not merely
        // because a panic flag became visible during an older batch's flush.
        if panic_in_batch {
            shared.signal.mark_panic_flushed();
        }
    }
    // The terminal marker is reserved budget: whatever happened above, the
    // file must end by saying whether the capture is complete and why.
    let (complete, reason) = terminal_state(shared, capped);
    let terminal = Record {
        kind: EventKind::TraceEnd,
        elapsed_us: elapsed,
        thread: WRITER_THREAD.into(),
        operation: None,
        fields: serde_json::to_vec(&serde_json::json!({"complete": complete, "reason": reason}))?,
    };
    let mut encoded = Vec::new();
    line(&mut encoded, seq + 1, &terminal)?;
    if encoded.len() > TERMINAL_RESERVE || encoded.len() > limits.max_bytes.saturating_sub(bytes) {
        return Err(io::Error::other("terminal reservation violated"));
    }
    out.write_all(&encoded)?;
    out.flush()
}

/// Only an explicit finish of an unfailed, uncapped, unpanicked capture
/// may ever be called complete; the reason names the first verdict that
/// applied.
fn terminal_state(shared: &Shared, capped: bool) -> (bool, &'static str) {
    if capped {
        return (false, "capture_limit");
    }
    if shared.panicked.load(std::sync::atomic::Ordering::Acquire) {
        return (false, "panic");
    }
    if shared.failure.present() {
        return (false, "capture_failure");
    }
    match shared.close_kind() {
        close::FINISH => (true, "complete"),
        _ => (false, "abandoned"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OperationId;
    use std::time::Duration;

    fn sample(operation: Option<OperationId>) -> Record {
        Record {
            kind: EventKind::Input,
            elapsed_us: 12,
            thread: "main-1".into(),
            operation,
            fields: br#"{"k":1}"#.to_vec(),
        }
    }

    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("disk full"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn disk_failure_is_recorded_and_signaled_not_silenced() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(sample(None)).unwrap();
        drop(tx);
        let shared = Arc::new(Shared::default());
        run_sink(FailingWriter, rx, Arc::clone(&shared), Limits::default());
        assert!(
            shared.failure.present(),
            "failure recorded for the terminal verdict"
        );
        assert!(
            shared
                .signal
                .wait_for(|state| state.done, Duration::from_millis(50)),
            "writer completion is still signaled"
        );
    }

    #[test]
    fn terminal_marker_always_writes_within_reservation() {
        let (tx, rx) = std::sync::mpsc::channel();
        drop(tx);
        let shared = Shared::default();
        shared
            .close_kind
            .store(close::FINISH, std::sync::atomic::Ordering::Relaxed);
        let mut out = Vec::new();
        drain(&mut out, rx, &shared, Limits::default()).unwrap();
        let text = String::from_utf8(out).unwrap();
        let line = text.trim();
        assert!(line.contains("\"event\":\"trace_end\""));
        assert!(line.contains("\"thread\":\"rootle-trace-writer\""));
        assert!(line.contains("\"complete\":true"));
        assert!(line.contains("\"reason\":\"complete\""));
        assert!(line.len() <= TERMINAL_RESERVE);
    }
}
