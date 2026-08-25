//! Fake-provider integration tests: this test binary re-executed as
//! the child. No python/sh dependency (the docker `test` stage has
//! neither): the parent spawns current_exe with a filter that runs
//! ONLY the `fake_provider_child` test, which loops on stdin replying
//! per ROOTLE_FAKE_PROVIDER's script until killed.

use super::StdioProvider;
use crate::provider::{ErrorKind, Provider, ProviderError};
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
        // The first message of every process generation is the
        // initialize handshake (respawns keep incrementing ids).
        if !handshaken {
            handshaken = true;
            writeln!(
                stdout,
                r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocol":1,"name":"fake"}}}}"#
            )
            .unwrap();
            stdout.flush().unwrap();
            continue;
        }
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
            "slow" => {
                std::thread::sleep(Duration::from_millis(200));
                writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#).unwrap();
                stdout.flush().unwrap();
            }
            _ => {
                writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#).unwrap();
                stdout.flush().unwrap();
            }
        }
    }
}

/// Spawn the fake provider in `mode` with a short test deadline.
/// The child is this test binary re-executed with a filter that
/// runs only `fake_provider_child`; the mode travels via env.
fn fake(mode: &str, timeout: Duration) -> StdioProvider {
    let exe = std::env::current_exe().expect("test binary path");
    let argv = vec![
        exe.to_string_lossy().into_owned(),
        "provider::stdio::tests::fake_provider_child".to_string(),
        "--exact".to_string(),
        "--nocapture".to_string(),
    ];
    StdioProvider::spawn_with_env(&argv, timeout, &[("ROOTLE_FAKE_PROVIDER", mode)])
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
    assert_eq!(err.message, "plain");
}

#[test]
fn restart_recovers_after_child_death() {
    let provider = fake("die-on-2", Duration::from_secs(5));
    // Generation 1 dies serving id 2.
    provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("dead child must fail the request");
    // The NEXT request respawns (1s backoff), re-handshakes, and
    // proceeds — id allocation keeps climbing, so the new child
    // never sees a colliding id.
    let start = std::time::Instant::now();
    provider
        .request("org/repos", json!({ "org": "o" }))
        .expect("respawn must recover the transport");
    assert!(
        start.elapsed() >= Duration::from_secs(1),
        "first restart pays the 1s backoff"
    );
    let notice = provider.take_notice().expect("restart leaves a notice");
    assert!(
        notice.contains("provider restarted"),
        "notice names the restart, got: {notice}"
    );
    assert!(provider.take_notice().is_none(), "notice is one-shot");
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
    // Give A's line time to reach the child so it holds id 2 back.
    std::thread::sleep(Duration::from_millis(150));
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

/// R1/R2: a request arriving while a rebuild is in flight waits on
/// the condvar (not the mutex), and proceeds only once the replacement
/// child has PASSED the initialize handshake.
#[test]
fn waiter_during_rebuild_proceeds_on_validated_transport() {
    let provider = Arc::new(fake("die-on-2", Duration::from_secs(5)));
    // Kill generation 1 (id 2).
    provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("dead child must fail the request");

    // The rebuilder: discovers Dead, sleeps the 1s backoff, spawns
    // generation 2, handshakes.
    let rebuilder = {
        let p = Arc::clone(&provider);
        std::thread::spawn(move || {
            p.request("org/repos", json!({ "org": "o" }))
                .expect("rebuild must recover the transport");
        })
    };
    // Rebuilder is now parked in its backoff; this caller must see
    // Respawning and WAIT for the validated transport, not fail, not
    // run against the unvalidated child.
    std::thread::sleep(Duration::from_millis(200));
    let waiter_start = std::time::Instant::now();
    provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect("waiter must succeed on the rebuilt transport");
    rebuilder.join().expect("rebuilder panics");
    assert!(
        waiter_start.elapsed() < Duration::from_secs(4),
        "waiter rides the in-flight rebuild instead of chaining sleeps"
    );
}

/// R1: a waiter that wakes to a FAILED rebuild fails immediately with
/// the attempt's reason — it must not start (and sleep through) a
/// second backoff ladder of its own. Drives the state machine
/// directly so the timing is deterministic.
#[test]
fn failed_rebuild_fails_waiters_without_resleeping() {
    use crate::provider::ErrorKind;

    let provider = Arc::new(fake("die-on-2", Duration::from_secs(5)));
    // Kill generation 1 so no live child interferes.
    provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("dead child must fail the request");

    // Park the machine in Respawning, then fail the attempt from a
    // side thread — exactly what a rebuild whose handshake fails does.
    {
        let mut routing = provider.shared.routing.lock();
        assert_eq!(routing.lifecycle, super::transport::Lifecycle::Dead);
        routing.lifecycle = super::transport::Lifecycle::Respawning;
    }
    let notifier = {
        let p = Arc::clone(&provider);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let err = crate::provider::ProviderError::new(ErrorKind::Provider, "handshake boom");
            p.finish_rebuild(1, Err(&err));
        })
    };

    let start = std::time::Instant::now();
    let err = provider
        .ensure_alive()
        .expect_err("waiter must fail when the rebuild fails");
    notifier.join().unwrap();
    assert_eq!(err.kind, ErrorKind::Provider);
    assert!(
        err.message.contains("handshake boom"),
        "waiter sees the attempt's stored reason, got: {err}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "waiter woke and failed fast (no second backoff sleep); took {:?}",
        start.elapsed()
    );
}

/// The one rebuild failure mode the fixtures can't reach organically
/// (every fake reuses this test binary as the provider, which always
/// exists): the spawn itself failing. The outcome must publish like
/// every other failure — a spawn error that returns early leaves the
/// machine in Respawning forever, and every future caller parks on the
/// condvar with nobody left to signal it.
#[test]
fn failed_spawn_publishes_failure_and_stays_recoverable() {
    let mut provider = fake("die-on-2", Duration::from_secs(5));
    // Kill generation 1 so the next request rebuilds.
    provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("dead child must fail the request");

    // Now break the command: the rebuild attempt can't even spawn.
    provider.command = vec!["/nonexistent/rootle-test-provider".into()];
    let err = provider
        .request("org/repos", json!({ "org": "o" }))
        .expect_err("unspawneable provider must fail the rebuild");
    assert!(
        err.message.contains("spawn"),
        "expected the spawn error, got: {err}"
    );
    let routing = provider.shared.routing.lock();
    assert_eq!(
        routing.lifecycle,
        super::transport::Lifecycle::Dead,
        "a failed spawn must land in Dead (recoverable), not wedge in Respawning"
    );
    assert!(
        routing
            .restart_error
            .as_deref()
            .is_some_and(|e| e.contains("spawn")),
        "waiters must see the spawn failure as the stored reason"
    );
}

/// The Drop guard: a rebuild sleeping in backoff when the provider is
/// dropped must not spawn a replacement child (nobody would ever kill
/// it) and must still publish its outcome so waiters wake and fail.
#[test]
fn closed_provider_rebuild_does_not_spawn_and_publishes() {
    use crate::provider::ErrorKind;

    let provider = Arc::new(fake("die-on-2", Duration::from_secs(5)));
    // Kill generation 1, then simulate drop-mid-recovery.
    provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("dead child must fail the request");
    provider
        .closed
        .store(true, std::sync::atomic::Ordering::Release);

    let err = provider
        .rebuild(1)
        .expect_err("closed provider must fail the rebuild");
    assert_eq!(err.kind, ErrorKind::Other);
    assert!(err.message.contains("dropped"), "got: {err}");
    let routing = provider.shared.routing.lock();
    assert_eq!(
        routing.lifecycle,
        super::transport::Lifecycle::Dead,
        "closed rebuild must land in Dead, not wedge in Respawning"
    );
    assert!(
        routing
            .restart_error
            .as_deref()
            .is_some_and(|e| e.contains("dropped")),
        "the outcome is published so parked waiters wake and fail"
    );
}
