use super::*;

#[test]
fn version_ordering() {
    // One minor up from the running crate version is always newer.
    let cur = env!("CARGO_PKG_VERSION");
    let mut parts = cur.split('.').map(|p| p.parse::<u64>().unwrap());
    let next = format!(
        "v{}.{}.{}",
        parts.next().unwrap(),
        parts.next().unwrap() + 1,
        parts.next().unwrap()
    );
    assert!(is_newer(&next));
    assert!(!is_newer("v0.1.0"));
    assert!(!is_newer(cur));
    assert!(!is_newer("garbage"));
}

/// 0018 M2: the toast nags once per version per 24h, then goes
/// quiet (the chip persists regardless).
#[test]
fn toast_is_once_per_version_per_day() {
    let dir = std::env::temp_dir().join(format!("rootle-toast-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("update.json");

    assert!(take_toast_at(&path, "v9.9.9", 1_000), "first sight nags");
    assert!(
        !take_toast_at(&path, "v9.9.9", 1_000 + 3600),
        "same day is quiet"
    );
    assert!(
        take_toast_at(&path, "v9.9.9", 1_000 + DAY + 1),
        "a day later nags again"
    );
    assert!(
        take_toast_at(&path, "v9.9.10", 1_000 + DAY + 2),
        "a new version nags immediately"
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("shown_at"), "stamp carries shown_at: {text}");
    assert!(text.contains("v9.9.10"), "stamp tracks the new tag: {text}");
}

/// 0018 M2: CI, dumb terminals, and piped stdout never check.
#[test]
fn check_environment_gates() {
    assert!(check_allowed_from(None, Some("xterm-256color"), true));
    assert!(check_allowed_from(Some("false"), Some("xterm"), true));
    assert!(
        !check_allowed_from(Some("true"), None, true),
        "CI=true skips"
    );
    assert!(!check_allowed_from(Some("1"), None, true), "CI=1 skips");
    assert!(
        !check_allowed_from(None, Some("dumb"), true),
        "dumb TERM skips"
    );
    assert!(!check_allowed_from(None, None, false), "piped stdout skips");
}

/// 0018 M3: the exit line fires only when disk > running.
#[test]
fn exit_line_only_when_disk_is_newer() {
    assert_eq!(
        exit_note("0.8.2", "0.8.3").as_deref(),
        Some("v0.8.3 installed — relaunch for it")
    );
    assert_eq!(exit_note("0.8.3", "0.8.3"), None, "same version is silent");
    assert_eq!(
        exit_note("0.8.3", "v0.9.0").as_deref(),
        Some("v0.9.0 installed — relaunch for it")
    );
    assert_eq!(exit_note("0.8.3", "0.8.2"), None, "older disk is silent");
    assert_eq!(exit_note("0.8.3", "garbage"), None);
}

/// 0017 M2 end to end against a loopback release + 0018 M1's step
/// sequence: resolved → downloading → verified → extracted →
/// swapped → summary → changelog note; a tampered sidecar refuses
/// and leaves the target intact.
#[test]
fn tarball_update_downloads_verifies_and_swaps() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let target = rootle_manager::platform_target();
    let file = format!("rootle-9.9.9-{target}.tar.gz");
    let payload = b"#!/bin/sh\necho new rootle\n";

    // pkg/rootle tarball, the release-asset shape.
    let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(enc);
    let mut header = tar::Header::new_gnu();
    header.set_size(payload.len() as u64);
    header.set_mode(0o755);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    builder
        .append_data(&mut header, "pkg/rootle", &payload[..])
        .unwrap();
    let tarball = builder.into_inner().unwrap().finish().unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        Sha256::digest(&tarball)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    let base = server.uri();
    rt.block_on(async {
        Mock::given(method("GET"))
            .and(path("/repos/rootledev/rootle/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v9.9.9",
                "assets": [
                    {"name": file, "browser_download_url": format!("{base}/dl/{file}")},
                    {"name": format!("{file}.sha256"), "browser_download_url": format!("{base}/dl/{file}.sha256")},
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/dl/{file}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/dl/{file}.sha256")))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(format!("{sha}  {file}")),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/dl/tampered-{file}.sha256")))
            .respond_with(ResponseTemplate::new(200).set_body_string("deadbeef  x"))
            .mount(&server)
            .await;
    });

    let dir = std::env::temp_dir().join(format!("rootle-update-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("rootle");
    std::fs::write(&exe, b"#!/bin/sh\necho old\n").unwrap();

    let (ui, log) = rootle_manager::progress::ProgressOutput::recorder();
    let out = update_inner(&base, false, &exe, Channel::Tarball, &ui).expect("update");
    assert_eq!(out, None, "the Ui already said it all");
    assert_eq!(std::fs::read(&exe).unwrap(), payload, "swapped in place");
    assert!(
        !dir.join("rootle.update-tmp").exists(),
        "staging file is renamed away"
    );

    // 0018 M1: the manager's step grammar, in order.
    let lines = log.lock().unwrap().clone();
    let expect = [
        "Updating rootle".to_string(),
        " ✓ Resolved v9.9.9".to_string(),
        format!(" ● Downloading rootle-9.9.9-{target}.tar.gz…"),
        " ● Verifying sha256 checksum…".to_string(),
        " ✓ Verified sha256 ok".to_string(),
        " ● Extracting rootle…".to_string(),
        " ✓ Extracted rootle".to_string(),
        format!(" ✓ Swapped {}", exe.display()),
    ];
    assert_eq!(&lines[..expect.len()], &expect, "step sequence");
    let current = env!("CARGO_PKG_VERSION");
    assert!(
        lines[expect.len()].starts_with(&format!(" ✓ Updated {current} → 9.9.9")),
        "summary line: {lines:?}"
    );
    assert_eq!(
        lines[expect.len() + 1],
        " ▸ takes effect on next launch · what's new: rootle.dev/changelog#999",
        "changelog note"
    );
    assert_eq!(lines.len(), expect.len() + 2, "no stray lines: {lines:?}");

    // A payload that doesn't match the served sidecar refuses.
    let err =
        rootle_manager::verify_checksum(b"not the tarball", &format!("{base}/dl/{file}.sha256"))
            .unwrap_err()
            .to_string();
    assert!(err.contains("checksum mismatch"), "got: {err}");

    // --check writes nothing and renders nothing.
    let exe3 = dir.join("rootle3");
    std::fs::write(&exe3, b"#!/bin/sh\necho old\n").unwrap();
    let (ui3, log3) = rootle_manager::progress::ProgressOutput::recorder();
    let line = update_inner(&base, true, &exe3, Channel::Tarball, &ui3).expect("check");
    assert!(
        line.as_deref().unwrap_or_default().contains("available"),
        "got: {line:?}"
    );
    assert_eq!(std::fs::read(&exe3).unwrap(), b"#!/bin/sh\necho old\n");
    assert!(log3.lock().unwrap().is_empty(), "check renders no steps");
}

/// 0019 M1: outcome rows render honestly — pinned and
/// install-and-pin say so, failures carry their error, and the
/// summary counts everything (upgraded rows come from the stage
/// blocks install_inner already rendered).
#[test]
fn sweep_rows_render_honestly() {
    let (ui, log) = rootle_manager::progress::ProgressOutput::recorder();
    render_sweep(
        &[
            rootle_manager::SweepOutcome::Upgraded {
                name: "live".into(),
                from: "v0.1.0".into(),
                to: "v0.2.0".into(),
            },
            rootle_manager::SweepOutcome::Current {
                name: "bb".into(),
                tag: "v0.1.4".into(),
            },
            rootle_manager::SweepOutcome::Pinned {
                name: "internal".into(),
                tag: "v0.3.0".into(),
            },
            rootle_manager::SweepOutcome::Untracked {
                name: "artifact".into(),
                source: "https://artifacts.corp/x.tar.gz".into(),
            },
            rootle_manager::SweepOutcome::Failed {
                name: "dead".into(),
                error: "network: refused".into(),
            },
        ],
        &ui,
        std::time::Duration::from_millis(50),
    );
    let lines = log.lock().unwrap().clone();
    assert_eq!(
        lines,
        vec![
            " · bb  v0.1.4 current",
            " 📌 internal  v0.3.0 pinned — skipped",
            " · artifact  https://artifacts.corp/x.tar.gz install-and-pin — untouched",
            " ✗ dead  network: refused",
            // <100ms: no timing suffix.
            " ✓ Swept providers 1 upgraded · 1 current · 1 pinned · 1 install-and-pin · 1 failed",
        ],
        "rows: {lines:?}"
    );
}
