//! The app↔provider seam over a REAL stdio child (moved from the
//! rootle-stdio crate, plans/0024 M5): `run_view_search` must pump
//! every `$/partial` batch through its sink before the metadata
//! final — catches plumbing loss between transport and view.
//!
//! Same trick as the stdio crate's fake: this test binary re-executed
//! as the child (docker `test` has no python/sh). The parent spawns
//! current_exe filtered to run ONLY `fake_provider_child`, which
//! loops on stdin replying per ROOTLE_FAKE_PROVIDER's script.

use rootle::components::global_search::{RawHit, SearchKind, run_view_search};
use rootle_stdio::StdioProvider;
use std::io::{BufRead, Write};
use std::time::Duration;

#[test]
fn fake_provider_child() {
    let Ok(mode) = std::env::var("ROOTLE_FAKE_PROVIDER") else {
        return;
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.expect("child read");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        let (id, method) = (
            v["id"].as_u64().unwrap_or(0),
            v["method"].as_str().unwrap_or(""),
        );
        if method == "initialize" {
            writeln!(
                stdout,
                r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocol":1,"name":"fake"}}}}"#
            )
            .unwrap();
            stdout.flush().unwrap();
            continue;
        }
        // v1.3 (plans/0011): search/code streams two $/partial
        // batches keyed by the request id, then a metadata-only
        // reply carrying `truncated`.
        if mode == "stream-search" && method == "search/code" {
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
            continue;
        }
        writeln!(stdout, r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#).unwrap();
        stdout.flush().unwrap();
    }
}

fn fake(mode: &str) -> StdioProvider {
    let exe = std::env::current_exe().expect("test binary path");
    let argv = vec![
        exe.to_string_lossy().into_owned(),
        "--exact".to_string(),
        "--nocapture".to_string(),
        "fake_provider_child".to_string(),
    ];
    StdioProvider::spawn_with_env(
        &argv,
        Duration::from_secs(10),
        &[("ROOTLE_FAKE_PROVIDER", mode)],
    )
    .expect("fake provider spawns + initializes")
}
/// The full backend seam over a streaming child: `run_view_search`
/// must pump every `$/partial` batch through its sink before the
/// metadata final (catches plumbing loss between provider and view).
#[test]
fn backend_streams_fake_provider_batches_through_the_sink() {
    let provider = fake("stream-search");
    let batches = std::sync::atomic::AtomicUsize::new(0);
    let outcome = run_view_search(
        &provider,
        SearchKind::Grep,
        "hit",
        "global",
        "",
        &|_hits: Vec<RawHit>| {
            batches.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        },
    )
    .expect("streamed search succeeds");
    assert_eq!(batches.load(std::sync::atomic::Ordering::Relaxed), 2);
    assert!(outcome.clipped); // the fake replies truncated: true
}
