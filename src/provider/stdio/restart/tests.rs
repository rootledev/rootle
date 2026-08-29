//! Recovery-machine tests (plans/0008 §5): the fixture-driven ones
//! ride the `die-on-2` fake child from `super::super::tests`; the
//! state-machine ones drive `rebuild`/`RebuildGuard` directly so the
//! timing is deterministic.

use crate::provider::stdio::tests::fake;
use crate::provider::stdio::transport::Lifecycle;
use crate::provider::{ErrorKind, Provider};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

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
    let provider = Arc::new(fake("die-on-2", Duration::from_secs(5)));
    // Kill generation 1 so no live child interferes.
    provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("dead child must fail the request");

    // Park the machine in Respawning, then fail the attempt from a
    // side thread — exactly what a rebuild whose handshake fails does.
    {
        let mut routing = provider.shared.routing.lock();
        assert_eq!(routing.lifecycle, Lifecycle::Dead);
        routing.lifecycle = Lifecycle::Respawning;
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
        Lifecycle::Dead,
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
        Lifecycle::Dead,
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

/// The structural guarantee: any exit from rebuild() that doesn't
/// disarm the guard — today only a panic — still resolves Respawning.
/// Drives the guard directly (panic injection isn't reachable here).
#[test]
fn armed_rebuild_guard_publishes_failure_on_drop() {
    let provider = fake("die-on-2", Duration::from_secs(5));
    provider
        .request("repo/tree", json!({ "repo": "o/r" }))
        .expect_err("dead child must fail the request");
    {
        let mut routing = provider.shared.routing.lock();
        assert_eq!(routing.lifecycle, Lifecycle::Dead);
        routing.lifecycle = Lifecycle::Respawning;
    }
    // An "exit path" that never calls finish_rebuild: drop the armed
    // guard, exactly what unwinding out of rebuild() does.
    let guard = super::RebuildGuard {
        provider: &provider,
        attempt: 1,
        armed: true,
    };
    drop(guard);
    let routing = provider.shared.routing.lock();
    assert_eq!(
        routing.lifecycle,
        Lifecycle::Dead,
        "an aborted rebuild must resolve Respawning, not wedge it"
    );
    assert!(
        routing
            .restart_error
            .as_deref()
            .is_some_and(|e| e.contains("aborted")),
        "waiters must see the abort as the stored reason"
    );
}

#[test]
fn backoff_ladder() {
    assert_eq!(super::backoff_for(1), Duration::from_secs(1));
    assert_eq!(super::backoff_for(2), Duration::from_secs(2));
    assert_eq!(super::backoff_for(3), Duration::from_secs(5));
    assert_eq!(super::backoff_for(9), Duration::from_secs(30));
}

/// 0022 M1: a provider that dies on every spawn notices the failure
/// streak ONCE — the sticky degraded surface, never per attempt.
#[test]
fn failure_streak_notices_once() {
    // "die-on-reinit": the child exits before the handshake — every
    // rebuild fails its proof. Construction itself handshakes lazily,
    // so the machine only discovers death on the first request.
    let provider = fake("die-on-reinit", Duration::from_secs(1));
    // Two independent requests = two rebuild attempts, one notice.
    let _ = provider.request("org/repos", json!({ "org": "o" }));
    let _ = provider.request("org/repos", json!({ "org": "o" }));
    let notice = provider
        .take_notice()
        .expect("a failing streak leaves a notice");
    assert!(
        notice.contains("keeps failing to restart"),
        "streak notice: {notice}"
    );
    assert!(
        provider.take_notice().is_none(),
        "the streak notices once, not per attempt"
    );
}
