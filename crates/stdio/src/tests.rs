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
fn fake_provider_child() {
    let Ok(mode) = std::env::var("ROOTLE_FAKE_PROVIDER") else {
        return;
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut handshaken = false;

    // "swap-2-3": id 2's reply is held back until id 3 arrives, then
    // answered after it — replies arrive out of request order.
    let mut stashed: Option<u64> = None;
    for line in stdin.lock().lines() {
        let line = line.expect("child read");
        let id: u64 = serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"]
            .as_u64()
            .unwrap_or(0);
        let method: String = serde_json::from_str::<serde_json::Value>(&line).unwrap()["method"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if mode == "die-on-reinit" && method != "initialize" && handshaken {
            // gen-1 dies serving the first real request; every later
            // initialize (the rebuild's re-handshake) dies too.
            std::process::exit(0);
        }
        // initialize may arrive on every process generation AND again
        // when rootle re-handshakes with fresh advisory params — a
        // conforming provider answers each one (restart obligations).
        if method == "initialize" {
            // "die-on-reinit": the first initialize answers (the
            // constructor handshake), every later one dies — every
            // rebuild's handshake proof fails (0022 M1's streak).
            handshaken = true;
            // "die-on-reinit": only the FIRST initialize anywhere in
            // this test run answers (the constructor's); every later
            // generation's handshake proof fails. Cross-generation
            // state rides a counter file (each child is a fresh
            // process, so in-memory flags can't do it).
            if mode == "die-on-reinit" {
                let run_id = std::env::var("ROOTLE_FAKE_RUN_ID").unwrap_or_default();
                let counter = std::env::temp_dir().join(format!("rootle-die-reinit-{run_id}"));
                let n: u32 = std::fs::read_to_string(&counter)
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
                std::fs::write(&counter, (n + 1).to_string()).unwrap();
                if n >= 1 {
                    std::process::exit(0);
                }
            }
            let params: serde_json::Value = serde_json::from_str(&line).unwrap();
            if mode == "echo-init"
                && params
                    .get("params")
                    .is_some_and(|p| p.get("cache_bytes").is_some())
            {
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocol":1,"name":"fake","cache":{{"bytes":218}}}}}}"#
                )
                .unwrap();
            } else {
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocol":1,"name":"fake"}}}}"#
                )
                .unwrap();
            }
            stdout.flush().unwrap();
            continue;
        }
        let _ = handshaken;
        match mode.as_str() {
            // Out-of-order replies: hold id 2 back; when id 3 lands,
            // answer 3 first, then the stashed 2. The child is
            // strictly sequential — deferring is the only way its
            // replies can overtake.
            "swap-2-3" => {
                if id == 2 {
                    stashed = Some(id);
                } else {
                    writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#).unwrap();
                    stdout.flush().unwrap();
                    if let Some(held) = stashed.take() {
                        std::thread::sleep(Duration::from_millis(300));
                        writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{held},"result":{{}}}}"#)
                            .unwrap();
                        stdout.flush().unwrap();
                    }
                }
            }
            // Swallow id 2 without replying but keep serving later
            // requests — models a single lost/hung backend call.
            "hang-on-2" if id == 2 => {}
            "die-on-2" if id == 2 => std::process::exit(0),
            // Dies the moment it spawns: the handshake never lands —
            // every rebuild fails (0022 M1's failure streak).
            "die-now" => std::process::exit(0),
            // Error taxonomy: each id answers with a differently
            // kinded error (plans/0008 §2).
            "error-kinds" => {
                let (kind, extra) = match id {
                    2 => ("auth", ""),
                    3 => ("rate_limited", r#", "retry_after_s": 37"#),
                    4 => ("bogus-kind", ""),
                    _ => ("", ""),
                };
                if kind.is_empty() {
                    writeln!(
                        stdout,
                        r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":1,"message":"plain"}}}}"#
                    )
                    .unwrap();
                } else {
                    writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":1,"message":"{kind} happened","data":{{"kind":"{kind}"{extra}}}}}}}"#).unwrap();
                }
                stdout.flush().unwrap();
            }
            // v1.3 (plans/0011): search/code streams two $/partial
            // batches keyed by the request id, then a metadata-only
            // reply carrying `truncated`.
            "stream-search" if method == "search/code" => {
                for n in 1..=2 {
                    writeln!(
                        stdout,
                        r#"{{"jsonrpc":"2.0","method":"$/partial","params":{{"id":{id},"items":[{{"repo":"o/r","path":"f{n}.rs","sha":"s{n}","matches":["hit"],"line":{n}}}]}}}}"#
                    )
                    .unwrap();
                    stdout.flush().unwrap();
                }
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"items":[],"truncated":true}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            // One batch, then death: the caller keeps the partials and
            // the request fails (LSP #786 mid-stream error semantics).
            "die-mid-stream" if method == "search/code" => {
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","method":"$/partial","params":{{"id":{id},"items":[{{"repo":"o/r","path":"only.rs","sha":"s","matches":["hit"]}}]}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
                std::process::exit(0);
            }
            // Batches slower than the gap a short deadline tolerates
            // individually, longer than the deadline in total — proves
            // partials reset the inactivity deadline (v1.3).
            "stream-slow" if method == "search/code" => {
                for n in 1..=3 {
                    std::thread::sleep(Duration::from_millis(150));
                    writeln!(
                        stdout,
                        r#"{{"jsonrpc":"2.0","method":"$/partial","params":{{"id":{id},"items":[{{"repo":"o/r","path":"f{n}.rs","sha":"s{n}","matches":["hit"],"line":{n}}}]}}}}"#
                    )
                    .unwrap();
                    stdout.flush().unwrap();
                }
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"items":[],"truncated":false}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            "slow" => {
                std::thread::sleep(Duration::from_millis(200));
                writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#).unwrap();
                stdout.flush().unwrap();
            }
            // v1.4 richer org/repos: mixed string and object entries
            // in one reply — reader tolerance both directions.
            "rich-repos" if method == "org/repos" => {
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"repos":["plain",{{"name":"meta","description":"d","private":true,"archived":true,"pushed_at":"2026-08-20T10:11:12Z"}}]}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            "check-limit" if method == "search/code" => {
                let req: serde_json::Value = serde_json::from_str(&line).unwrap();
                let got =
                    req["params"]["limit"].as_u64() == Some(rootle_provider::RENDER_BUDGET as u64);
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"items":[],"truncated":{got}}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            // 0027 fault injection: the TLA model's fault classes as
            // wire events (specs/ProviderProtocol.tla invariants).
            "double-final" if id == 2 => {
                // UniqueTerminal/CorrelationSafety: two finals for one
                // id — the second must be dropped, never applied
                for which in 1..=2 {
                    writeln!(
                        stdout,
                        r#"{{"jsonrpc":"2.0","id":{id},"result":{{"which":{which}}}}}"#
                    )
                    .unwrap();
                    stdout.flush().unwrap();
                }
            }
            // PartialOrder: a $/partial AFTER the reply — out of
            // order, must be dropped (the slot died with the reply)
            "partial-after-final" if method == "search/code" => {
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"items":[],"truncated":false}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","method":"$/partial","params":{{"id":{id},"items":[{{"repo":"o/r","path":"late.rs","sha":"s","matches":["hit"]}}]}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            // CorrelationSafety: a reply for an id that was never
            // allocated — ignored, never fatal, before the real answer
            "unknown-id" => {
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":999,"result":{{"phantom":true}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"real":true}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            // NoIdReuse: hang id 2, answer it only alongside id 3 —
            // by then the client timed out; the late reply must be
            // dropped and can never satisfy the fresh id-3 slot
            "late-then-next" if id == 2 => {}
            "late-then-next" => {
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":2,"result":{{"late":true}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
                writeln!(
                    stdout,
                    r#"{{"jsonrpc":"2.0","id":{id},"result":{{"fresh":true}}}}"#
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            // RestartFailClosed: id 2 stays in flight, id 3's arrival
            // kills the child — EOF must fail BOTH callers at once
            "hang-2-die-3" => {
                if id == 3 {
                    std::process::exit(0);
                }
            }
            _ => {
                writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#).unwrap();
                stdout.flush().unwrap();
            }
        }
    }
}

/// The child also echoes the initialize params it received (mode
/// "echo-init"), so the handshake contract is observable end to end.
#[test]
fn initialize_carries_the_cache_budget_and_records_usage() {
    let exe = std::env::current_exe().expect("test binary path");
    let argv = vec![
        exe.to_string_lossy().into_owned(),
        "tests::fake_provider_child".to_string(),
        "--exact".to_string(),
        "--nocapture".to_string(),
    ];
    unsafe { std::env::set_var("ROOTLE_FAKE_PROVIDER", "echo-init") };
    let provider = StdioProvider::spawn_with_cache(
        &argv,
        Duration::from_secs(5),
        false,
        512 * 1024 * 1024,
        Some(std::path::PathBuf::from("/tmp/rootle-test-cache/gitlab")),
    )
    .expect("spawns + initializes");
    assert_eq!(provider.cache_usage(), Some(218), "reply usage recorded");
}

/// Spawn the fake provider in `mode` with a short test deadline.
/// The child is this test binary re-executed with a filter that
/// runs only `fake_provider_child`; the mode travels via env.
pub(super) fn fake(mode: &str, timeout: Duration) -> StdioProvider {
    let exe = std::env::current_exe().expect("test binary path");
    let argv = vec![
        exe.to_string_lossy().into_owned(),
        "tests::fake_provider_child".to_string(),
        "--exact".to_string(),
        "--nocapture".to_string(),
    ];
    let run_id = std::process::id().to_string();
    StdioProvider::spawn_with_env(
        &argv,
        timeout,
        &[
            ("ROOTLE_FAKE_PROVIDER", mode),
            ("ROOTLE_FAKE_RUN_ID", run_id.as_str()),
        ],
    )
    .expect("fake provider spawns + initializes")
}

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

/// v1.4 bounded compute: every `search/code` carries the client's
/// render budget as `limit` (doc/provider-protocol.md) — both the
/// one-shot and the progressive call.
#[test]
fn search_code_sends_the_render_budget_as_limit() {
    let provider = fake("check-limit", Duration::from_secs(2));
    let plain = provider.search_code("needle").expect("search succeeds");
    assert!(
        plain.truncated,
        "search/code must carry limit=RENDER_BUDGET"
    );
    let streamed = provider
        .search_code_progressive("needle", &|_| {})
        .expect("progressive search succeeds");
    assert!(
        streamed.truncated,
        "progressive search/code must carry limit=RENDER_BUDGET"
    );
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
