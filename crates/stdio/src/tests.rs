//! Fake-provider integration tests: this test binary re-executed as
//! the child. No python/sh dependency (the docker `test` stage has
//! neither): the parent spawns current_exe with a filter that runs
//! ONLY the `fake_provider_child` test, which loops on stdin replying
//! per ROOTLE_FAKE_PROVIDER's script until killed.

use super::StdioProvider;
use rootle_provider::{ErrorKind, Provider, ProviderError};
use serde_json::json;
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn timeout_fails_request_and_transport_recovers() {
    let provider = fake("hang-on-2", Duration::from_millis(300));
    let start = std::time::Instant::now();
    let err = provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("id 2 must time out");
    assert_eq!(err.kind, ErrorKind::Timeout);
    assert!(
        err.message.contains("timeout"),
        "expected a timeout message, got: {err}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "timeout must fire near the deadline, took {:?}",
        start.elapsed()
    );
    // The transport is unwedged: the next request (id 3) succeeds
    // even though id 2's reply never arrives.
    provider
        .request("org/repos", json!({ "org": "o" }))
        .expect("transport must recover after a timed-out request");
}

#[test]
fn child_death_fails_pending_immediately() {
    let provider = fake("die-on-2", Duration::from_secs(30));
    let start = std::time::Instant::now();
    let err = provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("dead child must fail the request");
    assert_eq!(
        err,
        ProviderError::new(ErrorKind::Provider, "provider closed its output")
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "EOF must fail fast, not at the deadline; took {:?}",
        start.elapsed()
    );
}

#[test]
fn slow_replies_within_the_deadline_succeed() {
    let provider = fake("slow", Duration::from_secs(2));
    provider
        .request("org/repos", json!({ "org": "o" }))
        .expect("200ms reply must land within a 2s deadline");
}

/// v1.4: `org/repos` entries are the bare name or a metadata object
/// — both forms parse, the object form carries its fields through.
#[test]
fn org_repos_accepts_the_v14_union() {
    let provider = fake("rich-repos", Duration::from_secs(2));
    let repos = provider.org_repos("o").expect("org/repos succeeds");
    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0], rootle_provider::RepoInfo::bare("plain"));
    let meta = &repos[1];
    assert_eq!(meta.name, "meta");
    assert_eq!(meta.description.as_deref(), Some("d"));
    assert!(meta.private && meta.archived);
    assert_eq!(meta.pushed_at.as_deref(), Some("2026-08-20T10:11:12Z"));
}

#[test]
fn error_data_kind_parses_into_the_taxonomy() {
    let provider = fake("error-kinds", Duration::from_secs(2));

    let err = provider.request("a", json!({})).expect_err("auth error");
    assert_eq!(err.kind, ErrorKind::Auth);
    assert_eq!(err.message, "auth happened");
    assert_eq!(err.retry_after, None);

    let err = provider.request("b", json!({})).expect_err("rate limited");
    assert_eq!(err.kind, ErrorKind::RateLimited);
    assert_eq!(err.retry_after, Some(Duration::from_secs(37)));

    // Unknown kinds degrade to Other; absent data does too.
    let err = provider.request("c", json!({})).expect_err("unknown kind");
    assert_eq!(err.kind, ErrorKind::Other);
    let err = provider.request("d", json!({})).expect_err("kindless");
    assert_eq!(err.kind, ErrorKind::Other);
}

/// v1.3 (plans/0011): `$/partial` batches arrive, in order, strictly
/// before the metadata-only reply; `truncated` rides the reply.
#[test]
fn streaming_search_delivers_ordered_batches_then_metadata() {
    let provider = fake("stream-search", Duration::from_secs(5));
    let batches = std::sync::Mutex::new(Vec::new());
    let result = provider
        .search_code_progressive("hit", &|items: &[rootle_provider::CodeMatch]| {
            batches.lock().unwrap().push(
                items
                    .iter()
                    .map(|m| (m.path.clone(), m.line))
                    .collect::<Vec<(String, Option<u32>)>>(),
            );
        })
        .unwrap();
    assert_eq!(
        batches.into_inner().unwrap(),
        vec![
            vec![("f1.rs".to_string(), Some(1))],
            vec![("f2.rs".to_string(), Some(2))],
        ],
        "ordered batches, provider-known lines (v1.3)"
    );
    assert!(result.hits.is_empty(), "streamed final is metadata-only");
    assert!(result.truncated);
}

/// Child death mid-stream: partials already delivered stay with the
/// caller, and the request itself fails.
#[test]
fn child_death_mid_stream_keeps_partials_and_fails() {
    let provider = fake("die-mid-stream", Duration::from_secs(5));
    let seen = std::sync::atomic::AtomicUsize::new(0);
    let err = provider
        .search_code_progressive("hit", &|_| {
            seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })
        .unwrap_err();
    assert_eq!(
        seen.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "the first batch should have been delivered"
    );
    assert_eq!(err.kind, ErrorKind::Provider);
}

/// Every `$/partial` resets the read deadline (inactivity semantics):
/// a stream whose total exceeds the deadline succeeds while no single
/// gap does.
#[test]
fn partials_reset_the_inactivity_deadline() {
    let provider = fake("stream-slow", Duration::from_millis(400));
    let batches = std::sync::atomic::AtomicUsize::new(0);
    provider
        .search_code_progressive("hit", &|_| {
            batches.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })
        .expect("stream should outlive the deadline via resets");
    assert_eq!(batches.load(std::sync::atomic::Ordering::Relaxed), 3);
}

/// S2, now load-bearing: two requests in flight at once, replies
/// arriving OUT of request order (id 3 answers before the stashed
/// id 2). The per-id reply slots must route each reply to its own
/// caller — B completes while A is still waiting.
#[test]
fn concurrent_requests_route_out_of_order_replies() {
    let provider = std::sync::Arc::new(fake("swap-2-3", Duration::from_secs(5)));
    let (tx, rx) = std::sync::mpsc::channel::<(&'static str, std::time::Instant)>();

    let a = {
        let provider = Arc::clone(&provider);
        let tx = tx.clone();
        std::thread::spawn(move || {
            provider
                .request("a/first", json!({}))
                .expect("request A (stashed reply) must still be answered");
            tx.send(("A", std::time::Instant::now())).unwrap();
        })
    };
    // Gate B on A's request actually being in flight — ids are
    // assigned in arrival order, and the fake stashes id 2. Without
    // this the two thread-starts race and the assertion inverts
    // under load.
    while provider
        .current_id
        .load(std::sync::atomic::Ordering::Acquire)
        == 0
    {
        std::thread::yield_now();
    }
    let b = {
        let provider = Arc::clone(&provider);
        std::thread::spawn(move || {
            provider
                .request("b/second", json!({}))
                .expect("request B must be answered immediately");
            tx.send(("B", std::time::Instant::now())).unwrap();
        })
    };
    a.join().expect("A panics");
    b.join().expect("B panics");

    let order: Vec<&str> = rx.iter().map(|(who, _)| who).collect();
    assert_eq!(
        order,
        vec!["B", "A"],
        "id-routed slots must deliver the fast reply (B) before the delayed one (A)"
    );
}

// -- 0027 bridge tests: every fault class the TLA model names, as a
// -- wire event against the real transport ------------------------------

/// UniqueTerminal/CorrelationSafety: a duplicate final for an already
/// answered id is dropped — the caller keeps the FIRST result, the
/// second is never applied, and the transport stays usable.
#[test]
fn duplicate_final_reply_is_dropped_not_applied() {
    let provider = fake("double-final", Duration::from_secs(5));
    let first = provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect("the first final must complete the request");
    assert_eq!(first["which"], 1, "the first reply wins");
    // The duplicate landed after the slot was removed; the follow-up
    // request proves the reader survived it.
    let next = provider
        .request("org/repos", json!({ "org": "o" }))
        .expect("a duplicate final must not wedge the reader");
    assert!(
        next.get("which").is_none(),
        "id 3 answers via the default arm"
    );
}

/// PartialOrder: a `$/partial` arriving after the reply is out of
/// order and must be dropped — the sink never sees it, and a second
/// streaming round trip (which forces the reader past the stray line)
/// still succeeds.
#[test]
fn partial_after_final_reply_is_dropped() {
    let provider = fake("partial-after-final", Duration::from_secs(5));
    let seen = std::sync::atomic::AtomicUsize::new(0);
    let count = || seen.load(std::sync::atomic::Ordering::Relaxed);
    for _ in 0..2 {
        provider
            .search_code_progressive("hit", &|_| {
                seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
            .expect("the reply must complete the request");
    }
    assert_eq!(
        count(),
        0,
        "no $/partial may be routed after its id's final reply"
    );
}

/// CorrelationSafety: a reply for an id that was never allocated is
/// ignored — the reader stays alive to deliver the real answer that
/// follows it on the same pipe.
#[test]
fn reply_for_unknown_id_is_ignored() {
    let provider = fake("unknown-id", Duration::from_secs(5));
    let result = provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect("the phantom reply must not stop the reader");
    assert_eq!(result["real"], true, "the real reply wins");
    provider
        .request("org/repos", json!({ "org": "o" }))
        .expect("transport stays usable after phantom replies");
}

/// NoIdReuse: id 2 times out; its reply arrives late, together with
/// id 3's request, and must be discarded — ids are monotonic and never
/// reused, so the late reply can never satisfy the fresh slot. Only a
/// transport that reuses ids would return `late: true` here.
#[test]
fn late_reply_after_timeout_is_discarded_not_rerouted() {
    let provider = fake("late-then-next", Duration::from_millis(300));
    let err = provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("the hung id 2 must time out");
    assert_eq!(err.kind, ErrorKind::Timeout);
    let next = provider
        .request("org/repos", json!({ "org": "o" }))
        .expect("the transport recovers, and the late reply is dropped");
    assert_eq!(
        next["fresh"], true,
        "id 3's slot must get id 3's reply — never the late id 2 one"
    );
}

/// RestartFailClosed: EOF fails EVERY in-flight request at once, not
/// just the one that raced the death — both callers fail fast with
/// the closed-pipe error, well before their deadlines.
#[test]
fn eof_fails_every_in_flight_request_at_once() {
    let provider = Arc::new(fake("hang-2-die-3", Duration::from_secs(30)));
    let (tx, rx) = std::sync::mpsc::channel::<ProviderError>();
    let a = {
        let provider = Arc::clone(&provider);
        let tx = tx.clone();
        std::thread::spawn(move || {
            tx.send(provider.request("a/hung", json!({})).unwrap_err())
                .unwrap();
        })
    };
    // Gate B on A actually waiting — ids are assigned in arrival
    while provider
        .current_id
        .load(std::sync::atomic::Ordering::Acquire)
        == 0
    {
        std::thread::yield_now();
    }
    let b = {
        let provider = Arc::clone(&provider);
        let tx = tx.clone();
        std::thread::spawn(move || {
            let err = provider.request("b/killer", json!({})).unwrap_err();
            tx.send(err).unwrap();
        })
    };
    drop(tx);
    a.join().expect("A panics");
    b.join().expect("B panics");
    let failures: Vec<ProviderError> = rx.iter().collect();
    assert_eq!(failures.len(), 2, "both in-flight requests must fail");
    for err in failures {
        assert_eq!(
            err,
            ProviderError::new(ErrorKind::Provider, "provider closed its output"),
            "EOF must fail-close every in-flight slot"
        );
    }
}

#[test]
fn unsolicited_partials_cannot_extend_a_nonstreaming_deadline() {
    let provider = fake("stream-slow", Duration::from_millis(250));
    let error = provider
        .search_code("hit")
        .expect_err("non-streaming calls retain their round-trip deadline");
    assert_eq!(error.kind, ErrorKind::Timeout);
}

#[test]
fn progressive_search_rebuilds_after_child_death() {
    let provider = fake("die-on-2", Duration::from_secs(5));
    provider
        .search_code_progressive("first", &|_| {})
        .expect_err("the first child dies");
    provider
        .search_code_progressive("second", &|_| {})
        .expect("progressive calls share the recovery gate");
}

// -- 0030 diagnostic tracing: privacy, byte accounting, lifecycle ------
//
// The recorder is process-global, so every test that installs a
// session holds this lock; other tests in this binary may emit
// records into an active session, which these assertions tolerate.
static TRACE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

fn trace_path(tag: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("rootle-stdio-trace-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

fn records(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .expect("trace file readable after finish")
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("trace file is JSONL"))
        .collect()
}

/// RpcMessage records carry the ACTUAL wire frame length (payload +
/// newline) and the routing outcome, and the default Metadata policy
/// never records request params or reply bodies — only this test's
/// own secret param value decides that, not method-name substrings.
#[test]
fn rpc_records_count_real_frame_bytes_and_exclude_payloads() {
    let _guard = TRACE_LOCK.lock();
    let path = trace_path("rpc-bytes");
    let session = rootle_trace::start(&path, rootle_trace::TraceOptions::default())
        .expect("trace session starts");
    let provider = fake("trace-bytes", Duration::from_secs(5));
    provider
        .request("org/repos", json!({ "org": "ORG-SECRET-VALUE" }))
        .expect("request round trip");
    drop(provider);
    session.finish().expect("trace session finishes");

    let expected = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "org/repos",
        "params": { "org": "ORG-SECRET-VALUE" }
    })
    .to_string()
    .len() as u64
        + 1; // the newline writeln! adds to the wire frame

    let events = records(&path);
    let tx = events.iter().any(|record| {
        record["event"] == "rpc_message"
            && record["fields"]["frame"] == "request"
            && record["fields"]["dir"] == "tx"
            && record["fields"]["method"] == "org/repos"
            && record["fields"]["id"] == 2
            && record["fields"]["written"] == true
            && record["fields"]["bytes"] == expected
    });
    assert!(
        tx,
        "tx record must report the exact frame byte count {expected}: {events:?}"
    );
    let rx = events.iter().any(|record| {
        record["event"] == "rpc_message"
            && record["fields"]["frame"] == "response"
            && record["fields"]["id"] == 2
            && record["fields"]["routed"] == "delivered"
            && record["fields"]["bytes"].as_u64().is_some_and(|b| b > 0)
    });
    assert!(rx, "rx record must report delivery and a real byte count");

    let file = std::fs::read_to_string(&path).unwrap();
    assert!(
        !file.contains("ORG-SECRET-VALUE"),
        "metadata policy must never record request params"
    );
    let _ = std::fs::remove_file(&path);
}

/// Provider stderr is captured only under the explicit Full policy
/// (Null mode; inherit is never diverted), the drainer is bounded and
/// keeps draining, and Drop terminates + joins it without hanging.
#[test]
fn provider_stderr_is_full_only_and_joins_cleanly_on_drop() {
    let _guard = TRACE_LOCK.lock();
    // Metadata: no stderr pipe at all — no provider_stderr records.
    let path = trace_path("stderr-meta");
    let session = rootle_trace::start(&path, rootle_trace::TraceOptions::default())
        .expect("metadata trace session starts");
    {
        let provider = fake("chatty-stderr", Duration::from_secs(5));
        provider
            .request("search/repos", json!({ "query": "q" }))
            .expect("request round trip under metadata");
        drop(provider);
    }
    session.finish().expect("metadata trace finishes");
    let meta_events = records(&path);
    assert!(
        meta_events
            .iter()
            .all(|record| record["event"] != "provider_stderr"),
        "metadata policy must not capture provider stderr"
    );
    let _ = std::fs::remove_file(&path);

    // Full: the null sink is replaced by a capture pipe; Drop joins
    // the drainer after killing the child, so it cannot hang.
    let path = trace_path("stderr-full");
    let session = rootle_trace::start(
        &path,
        rootle_trace::TraceOptions {
            content: rootle_trace::ContentPolicy::Full,
            ..rootle_trace::TraceOptions::default()
        },
    )
    .expect("full trace session starts");
    let dropped_at = std::time::Instant::now();
    {
        let provider = fake("chatty-stderr", Duration::from_secs(5));
        provider
            .request("search/repos", json!({ "query": "q" }))
            .expect("request round trip under full");
        drop(provider);
    }
    let drop_elapsed = dropped_at.elapsed();
    session.finish().expect("full trace finishes");
    assert!(
        drop_elapsed < Duration::from_secs(10),
        "Drop must terminate and join the stderr drainer promptly, took {drop_elapsed:?}"
    );
    let events = records(&path);
    let marker = events.iter().any(|record| {
        record["event"] == "provider_stderr"
            && record["fields"]["text"]
                .as_str()
                .is_some_and(|text| text.contains("rootle-fake-stderr-marker"))
    });
    assert!(marker, "full policy captures the provider's stderr text");
    let _ = std::fs::remove_file(&path);
}
pub(crate) mod support;
pub(crate) use support::fake;
