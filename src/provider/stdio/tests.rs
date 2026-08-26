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
        let method: String = serde_json::from_str::<serde_json::Value>(&line).unwrap()["method"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        // initialize may arrive on every process generation AND again
        // when rootle re-handshakes with fresh advisory params — a
        // conforming provider answers each one (restart obligations).
        if method == "initialize" {
            handshaken = true;
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

/// The child also echoes the initialize params it received (mode
/// "echo-init"), so the handshake contract is observable end to end.
#[test]
fn initialize_carries_the_cache_budget_and_records_usage() {
    let exe = std::env::current_exe().expect("test binary path");
    let argv = vec![
        exe.to_string_lossy().into_owned(),
        "provider::stdio::tests::fake_provider_child".to_string(),
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
