//! Re-executed stdio test child; no host Python or shell required.

use super::*;

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
            // 0030 trace regressions: a stderr marker line before each
            // reply — visible only to a Full-content capture pipe.
            "chatty-stderr" => {
                eprintln!("rootle-fake-stderr-marker id={id}");
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
pub(crate) fn fake(mode: &str, timeout: Duration) -> StdioProvider {
    let exe = std::env::current_exe().expect("test binary path");
    let argv = vec![
        exe.to_string_lossy().into_owned(),
        "tests::support::fake_provider_child".to_string(),
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
