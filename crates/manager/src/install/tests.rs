use super::*;

#[test]
fn only_releases_api_sources_are_tracked() {
    assert!(tracks_releases("rootledev/rootle-gitlab"));
    assert!(!tracks_releases(
        "https://artifacts.corp.example/p/rootle-gitlab.tar.gz"
    ));
    assert!(!tracks_releases("/opt/providers/rootle-gitlab"));
}

/// A manager rooted at a throwaway dir — never the real XDG store.
fn test_manager(tag: &str) -> Manager {
    let root = std::env::temp_dir().join(format!("rootle-mgr-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    Manager::rooted_at(root.join("store"), root.join("state"))
}

/// `pkg/<binary>` as a gzip'd tarball, the release-asset shape.
fn tarball_with(binary_name: &str, bytes: &[u8]) -> Vec<u8> {
    let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(enc);
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o755);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    builder
        .append_data(&mut header, format!("pkg/{binary_name}"), bytes)
        .unwrap();
    let enc = builder.into_inner().unwrap();
    enc.finish().unwrap()
}

/// plans/0014 #1a: the download/verify path is host-agnostic — a
/// plain-HTTP artifact host (here: loopback wiremock, not
/// github.com) gets the same verified install as a release.
#[test]
fn plain_http_install_downloads_verifies_and_pins() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let target = super::super::release::platform_target();
    let file = format!("rootle-gitlab-0.1.0-{target}.tar.gz");
    let tarball = tarball_with("rootle-gitlab", b"#!/bin/sh\necho fake\n");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(async {
        Mock::given(method("GET"))
            .and(path(format!("/providers/{file}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/providers/{file}.sha256")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(format!("{}  {file}", sha256_hex(&tarball))),
            )
            .mount(&server)
            .await;
        // A tampered twin: same layout, wrong sidecar.
        Mock::given(method("GET"))
            .and(path(format!("/providers/tampered-{file}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/providers/tampered-{file}.sha256")))
            .respond_with(ResponseTemplate::new(200).set_body_string("deadbeef  x"))
            .mount(&server)
            .await;
    });

    let manager = test_manager("http-install");
    let url = format!("{}/providers/{file}", server.uri());
    let r = ProviderReference::parse(&url).unwrap();
    assert_eq!(r.tarball.as_deref(), Some(url.as_str()));

    let receipt = manager.install(&r, false).expect("verified install");
    assert!(receipt.pinned, "plain-HTTP installs are install-and-pin");
    assert_eq!(receipt.source, url);
    assert_eq!(receipt.tag, "v0.1.0");
    assert_eq!(receipt.sha256, sha256_hex(&tarball));
    assert_eq!(receipt.latest_tag, None);
    let bin = manager.current_binary("gitlab").expect("current resolves");
    assert_eq!(std::fs::read(&bin).unwrap(), b"#!/bin/sh\necho fake\n");

    // Same URL, same receipt: idempotent refusal without --force.
    let again = manager.install(&r, false).unwrap_err().to_string();
    assert!(again.contains("already installed"), "got: {again}");

    // The tampered twin fails verification and leaves no receipt.
    let bad_url = format!("{}/providers/tampered-{file}", server.uri());
    let bad = ProviderReference::parse(&bad_url).unwrap();
    let err = manager.install(&bad, false).unwrap_err().to_string();
    assert!(err.contains("checksum mismatch"), "got: {err}");
    assert!(manager.receipt("tampered-gitlab").is_none());

    // #1b: update/upgrade never touch plain-HTTP receipts — no
    // network call against a bogus releases URL, no state change.
    assert!(manager.update(None).unwrap().is_empty());
    manager.upgrade(None, false, true).unwrap();
    assert_eq!(manager.receipt("gitlab").unwrap().tag, "v0.1.0");
}

/// 0019 M1: the sweep upgrades tracked receipts through the full
/// verified flow, reports pinned and install-and-pin sources
/// untouched, isolates a dead forge per provider, and `--check`
/// swaps nothing.
#[test]
fn sweep_upgrades_reports_and_isolates() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let target = platform_target();
    let file = format!("rootle-live-0.2.0-{target}.tar.gz");
    let tarball = tarball_with("rootle-live", b"#!/bin/sh\necho live 0.2.0\n");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(MockServer::start());
    let base = server.uri();
    rt.block_on(async {
        Mock::given(method("GET"))
            .and(path("/repos/acme/live/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v0.2.0",
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
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "{}  {file}",
                sha256_hex(&tarball)
            )))
            .mount(&server)
            .await;
        // acme/dead stays unmounted: every request 404s.
    });

    let manager = test_manager("sweep").with_api(&base);
    let seed = |name: &str, source: &str, tag: &str, pinned: bool| {
        manager
            .write_receipt(&Receipt {
                name: name.into(),
                source: source.into(),
                tag: tag.into(),
                sha256: "seeded".into(),
                pinned,
                installed_at: None,
                latest_tag: None,
            })
            .unwrap();
    };
    seed("live", "acme/live", "v0.1.0", false);
    seed("dead", "acme/dead", "v0.1.0", false);
    seed("pinned", "acme/live", "v0.1.0", true);
    seed(
        "artifact",
        "https://artifacts.corp/x.tar.gz",
        "v0.9.0",
        true,
    );

    let (ui, _log) = crate::progress::ProgressOutput::recorder();
    let outcomes = manager.sweep(false, &ui);
    // receipts() iterates sorted by name.
    assert!(matches!(&outcomes[0], SweepOutcome::Untracked { name, .. } if name == "artifact"));
    assert!(matches!(&outcomes[1], SweepOutcome::Failed { name, .. } if name == "dead"));
    assert!(
        matches!(&outcomes[2], SweepOutcome::Upgraded { name, from, to }
        if name == "live" && from == "v0.1.0" && to == "v0.2.0")
    );
    assert!(matches!(&outcomes[3], SweepOutcome::Pinned { name, tag }
        if name == "pinned" && tag == "v0.1.0"));

    // The upgraded provider landed atomically: versioned dir,
    // current pointer, executable payload, fresh receipt.
    let bin = manager.current_binary("live").expect("current resolves");
    assert_eq!(
        std::fs::read(&bin).unwrap(),
        b"#!/bin/sh\necho live 0.2.0\n"
    );
    assert_eq!(manager.receipt("live").unwrap().tag, "v0.2.0");
    assert!(
        manager.receipt("dead").unwrap().tag == "v0.1.0",
        "dead untouched"
    );

    // --check swaps nothing: a stale receipt reports Stale only.
    manager
        .write_receipt(&Receipt {
            name: "live".into(),
            source: "acme/live".into(),
            tag: "v0.1.0".into(),
            sha256: "seeded".into(),
            pinned: false,
            installed_at: None,
            latest_tag: Some("v0.2.0".into()),
        })
        .unwrap();
    let outcomes = manager.sweep(true, &ui);
    assert!(
        matches!(&outcomes[2], SweepOutcome::Stale { name, from, to }
        if name == "live" && from == "v0.1.0" && to == "v0.2.0"),
        "got: {outcomes:?}"
    );
    assert_eq!(
        manager.receipt("live").unwrap().tag,
        "v0.1.0",
        "dry run swaps nothing"
    );
}
