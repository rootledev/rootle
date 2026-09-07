//! Behavioral regressions for session storage and isolation. Tests that start
//! a session serialize their use of the global slot; each recorder owns its
//! own policy, failure and panic state.

use super::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

static SESSION: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Per-test scratch directory, mirroring the workspace's std-only test
/// convention (no tempfile dependency).
fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rootle-trace-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn parse(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn terminal(events: &[serde_json::Value]) -> &serde_json::Value {
    let last = events.last().expect("trace not empty");
    assert_eq!(
        last["event"], "trace_end",
        "every closed file ends with the terminal marker"
    );
    last
}

#[test]
fn lifecycle_is_exclusive_ordered_and_durable() {
    let _session = SESSION.lock();
    assert!(!enabled());
    let evaluated = std::cell::Cell::new(false);
    record_with(EventKind::Input, || {
        evaluated.set(true);
        serde_json::json!({"never": "built"})
    });
    assert!(!evaluated.get(), "disabled producers build no payload");
    assert_eq!(operation_id(), None, "no identity allocated while disabled");

    let dir = temp_root("lifecycle");
    let path = dir.join("session.jsonl");
    let session = start(&path, TraceOptions::default()).unwrap();
    assert!(enabled());
    assert!(!capture_content());
    assert!(matches!(
        start(&path, TraceOptions::default()),
        Err(TraceError::AlreadyActive)
    ));

    let operations: Vec<_> = (0..4).map(|_| operation_id().unwrap()).collect();
    std::thread::scope(|scope| {
        for (producer, operation) in operations.iter().enumerate() {
            let operation = *operation;
            let builder = std::thread::Builder::new().name(format!("producer-{producer}"));
            let spawned = builder
                .spawn(move || {
                    in_operation(Some(operation), || {
                        for value in 0..25 {
                            record(
                                EventKind::Input,
                                &serde_json::json!({"producer": producer, "value": value}),
                            );
                        }
                    });
                })
                .unwrap();
            let _ = scope.spawn(move || spawned.join().unwrap());
        }
    });
    session.finish().unwrap();
    assert!(!enabled());

    let events = parse(&path);
    assert_eq!(events.len(), 101, "100 records plus the terminal marker");
    let mut last_time = 0u64;
    let mut per_producer = [0u64; 4];
    let mut threads = HashSet::new();
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["schema_version"], 1);
        assert_eq!(event["seq"], index + 1, "sequence matches admission order");
        let timestamp = event["elapsed_us"].as_u64().unwrap();
        assert!(
            timestamp >= last_time,
            "timestamps agree with admission order"
        );
        last_time = timestamp;
        assert!(event["thread"].as_str().is_some_and(|t| !t.is_empty()));
        if event["event"] == "trace_end" {
            assert_eq!(index, 100, "terminal marker is the last record");
            assert_eq!(
                event["operation_id"],
                serde_json::Value::Null,
                "terminal carries no operation"
            );
            assert_eq!(event["thread"], "rootle-trace-writer");
            assert_eq!(event["fields"]["complete"], true);
            assert_eq!(event["fields"]["reason"], "complete");
            continue;
        }
        assert_eq!(event["event"], "input");
        let producer = event["fields"]["producer"].as_u64().unwrap() as usize;
        threads.insert(event["thread"].as_str().unwrap().to_string());
        assert_eq!(
            event["operation_id"], operations[producer].0,
            "worker records carry their operation identity"
        );
        assert_eq!(
            event["fields"]["value"], per_producer[producer],
            "records stay ordered within each producer"
        );
        per_producer[producer] += 1;
    }
    assert_eq!(threads.len(), 4, "each producer thread keeps one label");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "trace files are private"
        );
    }
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(
        matches!(
            start(&path, TraceOptions::default()),
            Err(TraceError::Open { .. })
        ),
        "existing files are refused, never appended"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert!(
        matches!(
            start(&dir.join("missing/child.jsonl"), TraceOptions::default()),
            Err(TraceError::Open { .. })
        ),
        "missing parents are errors; no directories are created"
    );

    // A finished session can be followed by a fresh one.
    let next = dir.join("next.jsonl");
    start(&next, TraceOptions::default())
        .unwrap()
        .finish()
        .unwrap();
    assert_eq!(terminal(&parse(&next))["fields"]["reason"], "complete");
}

#[test]
fn abandoned_session_is_explicitly_incomplete() {
    let _session = SESSION.lock();
    let dir = temp_root("abandon");
    let path = dir.join("abandoned.jsonl");
    let session = start(&path, TraceOptions::default()).unwrap();
    record_with(EventKind::State, || serde_json::json!({"mode": "normal"}));
    drop(session);
    assert!(!enabled(), "dropping deactivates the global producers");
    let events = parse(&path);
    assert_eq!(events.len(), 2, "record plus terminal marker");
    assert_eq!(events[0]["event"], "state");
    let terminal = terminal(&events);
    assert_eq!(terminal["fields"]["complete"], false);
    assert_eq!(terminal["fields"]["reason"], "abandoned");
}

#[test]
fn panic_capture_is_bounded_honest_and_content_gated() {
    let _session = SESSION.lock();
    let dir = temp_root("panic");
    let location = Some(("src/app.rs", 42u32, 7u32));

    let path = dir.join("metadata.jsonl");
    let session = start(&path, TraceOptions::default()).unwrap();
    record(
        EventKind::SessionStart,
        &serde_json::json!({"origin": "test"}),
    );
    let began = Instant::now();
    panic_record(
        location,
        Some("typed secret"),
        Some("frame zero\nframe one"),
    );
    let waited = began.elapsed();
    assert!(
        waited < Duration::from_secs(2),
        "healthy writer flushes the panic record promptly: {waited:?}"
    );
    let failure = session.finish();
    assert!(
        matches!(&failure, Err(TraceError::Incomplete(m)) if m.contains("panic")),
        "finish reports the interrupted capture: {failure:?}"
    );
    let events = parse(&path);
    let panic_line = &events[events.len() - 2];
    assert_eq!(panic_line["event"], "panic");
    assert_eq!(panic_line["fields"]["location"]["file"], "src/app.rs");
    assert_eq!(panic_line["fields"]["location"]["line"], 42);
    assert_eq!(panic_line["fields"]["location"]["column"], 7);
    assert_eq!(
        panic_line["fields"]["message"],
        serde_json::Value::Null,
        "raw panic text is Full-only even when supplied"
    );
    let marker = terminal(&events);
    assert_eq!(marker["fields"]["complete"], false);
    assert_eq!(marker["fields"]["reason"], "panic");

    // Full policy reveals the explicitly opted-in surfaces, still capped.
    let path = dir.join("full.jsonl");
    let session = start(
        &path,
        TraceOptions {
            content: ContentPolicy::Full,
            ..TraceOptions::default()
        },
    )
    .unwrap();
    assert!(capture_content());
    panic_record(
        location,
        Some("typed secret"),
        Some(&"frame\n".repeat(9000)),
    );
    assert!(session.finish().is_err());
    let events = parse(&path);
    let panic_line = &events[events.len() - 2];
    assert_eq!(panic_line["fields"]["message"], "typed secret");
    let backtrace = panic_line["fields"]["backtrace"].as_str().unwrap();
    assert!(backtrace.starts_with("frame"));
    assert!(
        backtrace.chars().count() <= PANIC_BACKTRACE_CHARS + 1,
        "backtrace capped for the record budget"
    );
    assert_eq!(terminal(&events)["fields"]["reason"], "panic");
}

#[test]
fn panic_admission_never_blocks_on_a_dead_producer_lock() {
    let _session = SESSION.lock();
    let dir = temp_root("panic-lock");
    let path = dir.join("held.jsonl");
    let session = start(&path, TraceOptions::default()).unwrap();
    let recorder = ACTIVE.load_full().expect("session active");
    let (held, is_held) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = recorder.sender.lock(); // a producer "died" holding admission
        let _ = held.send(());
        std::thread::sleep(Duration::from_millis(750));
    });
    is_held.recv().unwrap();
    let began = Instant::now();
    panic_record(Some(("x.rs", 1, 1)), Some("boom"), None);
    let waited = began.elapsed();
    assert!(
        waited < Duration::from_millis(500),
        "panic path must not wait on the admission lock: {waited:?}"
    );
    holder.join().unwrap();
    let failure = session.finish();
    assert!(
        matches!(&failure, Err(TraceError::Incomplete(m)) if m.contains("panic")),
        "the flag alone marks the capture incomplete: {failure:?}"
    );
    let events = parse(&path);
    assert_eq!(events.len(), 1, "the unadmittable panic record was skipped");
    let terminal = terminal(&events);
    assert_eq!(terminal["fields"]["complete"], false);
    assert_eq!(terminal["fields"]["reason"], "panic");
}

#[test]
fn event_cap_ends_with_explicit_incomplete_terminal_marker() {
    let _session = SESSION.lock();
    let dir = temp_root("cap");
    let path = dir.join("capped.jsonl");
    let session = start(
        &path,
        TraceOptions {
            limits: Limits {
                max_events: 5,
                ..Limits::default()
            },
            ..TraceOptions::default()
        },
    )
    .unwrap();
    for value in 0..50 {
        record(EventKind::Cache, &serde_json::json!({"value": value}));
    }
    let failure = session.finish();
    assert!(
        matches!(&failure, Err(TraceError::Incomplete(m)) if m.contains("capture limit")),
        "finish reports the cap: {failure:?}"
    );
    let events = parse(&path);
    assert_eq!(
        events.len(),
        6,
        "5 admitted events plus the terminal marker"
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], index + 1, "sequence stays contiguous");
    }
    let terminal = terminal(&events);
    assert_eq!(terminal["fields"]["complete"], false);
    assert_eq!(terminal["fields"]["reason"], "capture_limit");
}

#[test]
fn oversize_record_fails_visibly_and_stops_admission() {
    let _session = SESSION.lock();
    let dir = temp_root("oversize");
    let path = dir.join("oversize.jsonl");
    let session = start(&path, TraceOptions::default()).unwrap();
    record(
        EventKind::Input,
        &serde_json::json!({"blob": "x".repeat(MAX_RECORD_BYTES + 10)}),
    );
    let reported = take_failure().expect("oversize record reported");
    assert!(
        reported.contains("cap") || reported.contains("serialize"),
        "{reported}"
    );
    assert!(take_failure().is_none(), "failure is reported once");
    record(EventKind::State, &serde_json::json!({"after": true}));
    let failure = session.finish();
    assert!(
        matches!(&failure, Err(TraceError::Incomplete(_))),
        "finish still carries the failure: {failure:?}"
    );
    let events = parse(&path);
    assert_eq!(
        events.len(),
        1,
        "no partial line was written and admission stopped"
    );
    let terminal = terminal(&events);
    assert_eq!(terminal["fields"]["complete"], false);
    assert_eq!(terminal["fields"]["reason"], "capture_failure");
}

#[test]
fn queue_overflow_is_a_visible_failure_not_a_silent_drop() {
    let _session = SESSION.lock();
    // A disconnected receiver drives the same try_send failure arm a full
    // queue hits, without racing a real writer for capacity.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    drop(receiver);
    let recorder = Arc::new(Recorder {
        sender: Mutex::new(Some(sender)),
        shared: Arc::new(Shared::default()),
        started: Instant::now(),
        max_record: 4096,
        content: ContentPolicy::Metadata,
    });
    ACTIVE.store(Some(Arc::clone(&recorder)));
    SESSION_PRESENT.store(true, Ordering::Release);
    record(EventKind::Cache, &serde_json::json!({"hit": false}));
    let reported = take_failure().expect("queue overflow reported");
    assert!(reported.contains("queue full"), "{reported}");
    assert!(take_failure().is_none());
    assert!(
        recorder.sender.lock().is_none(),
        "admission is shut off after overflow"
    );
    let evaluated = std::cell::Cell::new(false);
    record_with(EventKind::Input, || {
        evaluated.set(true);
        serde_json::json!({"should_not_run":true})
    });
    assert!(
        !evaluated.get(),
        "failed capture must stop lazy payload construction"
    );
    SESSION_PRESENT.store(false, Ordering::Release);
    ACTIVE.store(None);
}

#[test]
fn invalid_limits_are_refused_before_any_file_exists() {
    let _session = SESSION.lock();
    let dir = temp_root("limits");
    let path = dir.join("never.jsonl");
    for limits in [
        Limits {
            max_bytes: 1024,
            ..Limits::default()
        }, // below writer+terminal floor
        Limits {
            max_bytes: MAX_CAPTURE_BYTES * 2,
            ..Limits::default()
        },
        Limits {
            max_events: 0,
            ..Limits::default()
        },
        Limits {
            max_events: MAX_CAPTURE_EVENTS + 1,
            ..Limits::default()
        },
        Limits {
            max_record_bytes: 0,
            ..Limits::default()
        },
        Limits {
            max_record_bytes: MAX_RECORD_BYTES + 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            start(
                &path,
                TraceOptions {
                    limits,
                    ..TraceOptions::default()
                }
            ),
            Err(TraceError::Incomplete(_))
        ));
    }
    assert!(!path.exists());
}

#[test]
fn operation_context_restores_through_unwind() {
    let _session = SESSION.lock();
    let dir = temp_root("operation");
    let path = dir.join("op.jsonl");
    let session = start(&path, TraceOptions::default()).unwrap();
    let outer = operation_id().expect("identity allocated while enabled");
    let inner = in_operation(Some(outer), || {
        assert_eq!(context::current_operation(), Some(outer));
        operation_id().expect("nested allocation works")
    });
    assert_ne!(outer, inner);
    assert_eq!(
        context::current_operation(),
        None,
        "identity restored after work"
    );

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        in_operation(Some(outer), || panic!("unwind through the operation"));
    }));
    std::panic::set_hook(previous_hook);
    assert!(result.is_err());
    assert_eq!(
        context::current_operation(),
        None,
        "identity restored after unwind"
    );
    session.finish().unwrap();
}

#[test]
fn shutdown_wait_is_bounded_not_hanging() {
    let signal = Signal::default();
    let began = Instant::now();
    assert!(
        !signal.wait_for(|state| state.done, Duration::from_millis(50)),
        "an unnotified writer times out instead of hanging"
    );
    assert!(began.elapsed() >= Duration::from_millis(40));
    signal.mark_done();
    assert!(signal.wait_for(|state| state.done, Duration::from_secs(5)));
    assert!(
        began.elapsed() < Duration::from_secs(1),
        "a signaled writer returns immediately"
    );
}

#[test]
fn prepared_full_payload_cannot_cross_into_a_later_metadata_session() {
    let _session = SESSION.lock();
    let dir = temp_root("session-isolation");
    let first_path = dir.join("full.jsonl");
    let first = start(
        &first_path,
        TraceOptions {
            content: ContentPolicy::Full,
            ..TraceOptions::default()
        },
    )
    .unwrap();
    let (prepared, ready) = std::sync::mpsc::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let producer = std::thread::spawn(move || {
        record_with(EventKind::State, || {
            let content = capture_content().then_some("FULL_SESSION_SECRET");
            prepared.send(()).unwrap();
            resumed.recv().unwrap();
            serde_json::json!({"text":content})
        });
    });
    ready.recv().unwrap();
    first.finish().unwrap();
    let second_path = dir.join("metadata.jsonl");
    let second = start(&second_path, TraceOptions::default()).unwrap();
    resume.send(()).unwrap();
    producer.join().unwrap();
    record_with(
        EventKind::State,
        || serde_json::json!({"second_session":true}),
    );
    second.finish().unwrap();
    let events = parse(&second_path);
    assert!(
        !std::fs::read_to_string(&second_path)
            .unwrap()
            .contains("FULL_SESSION_SECRET")
    );
    assert_eq!(events[0]["fields"]["second_session"], true);
    assert_eq!(terminal(&events)["fields"]["complete"], true);
}

#[test]
fn panic_state_and_policy_do_not_poison_the_next_session() {
    let _session = SESSION.lock();
    let dir = temp_root("panic-isolation");
    let first_path = dir.join("panic.jsonl");
    let first = start(
        &first_path,
        TraceOptions {
            content: ContentPolicy::Full,
            ..TraceOptions::default()
        },
    )
    .unwrap();
    panic_record(Some(("fixture.rs", 4, 2)), Some("FULL_PANIC_SECRET"), None);
    assert!(matches!(first.finish(), Err(TraceError::Incomplete(_))));
    assert_eq!(terminal(&parse(&first_path))["fields"]["reason"], "panic");
    let next_path = dir.join("next.jsonl");
    let next = start(&next_path, TraceOptions::default()).unwrap();
    record_with(EventKind::State, || serde_json::json!({"healthy":true}));
    next.finish().unwrap();
    let events = parse(&next_path);
    assert_eq!(events[0]["fields"]["healthy"], true);
    assert_eq!(terminal(&events)["fields"]["complete"], true);
    assert!(
        !std::fs::read_to_string(next_path)
            .unwrap()
            .contains("FULL_PANIC_SECRET")
    );
}
