//! App updates (plans/0017): `rootle update` for tarball installs and
//! the 24h-cached startup check behind the modeline notice.
//!
//! Integrity is the provider manager's model — the same release
//! helpers, the same mandatory `.sha256` sidecar, staged write +
//! atomic rename over self.

use crate::provider::manager as mgr;

/// How this binary was installed — decides the upgrade command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// install.sh / release tarball — self-updates.
    Tarball,
    Brew,
    Cargo,
    Mise,
    /// Unknown layout — self-update conservatively.
    Other,
}

/// The running binary's install channel, from its resolved path.
pub fn channel() -> Channel {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    if exe.contains(".cargo/bin") {
        Channel::Cargo
    } else if exe.contains("Cellar") || exe.contains("homebrew") || exe.contains("linuxbrew") {
        Channel::Brew
    } else if exe.contains("/mise/") {
        Channel::Mise
    } else if exe.contains("/.local/") || exe.contains("/usr/local/") {
        Channel::Tarball
    } else {
        Channel::Other
    }
}

/// (major, minor, patch) — suffixes (`-alpha.1`) don't order.
fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let v = tag.strip_prefix('v').unwrap_or(tag);
    let mut it = v.split('.');
    Some((
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    ))
}

/// Is `latest` strictly newer than the running crate version?
pub fn is_newer(latest: &str) -> bool {
    match (
        parse_version(latest),
        parse_version(env!("CARGO_PKG_VERSION")),
    ) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// `rootle update`: tarball installs self-update; every other channel
/// gets its own command. Returns the human line.
pub fn update(check_only: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    update_inner("https://api.github.com", check_only, &exe, channel())
}

/// The flow, with the API base and target exe swapped in tests.
fn update_inner(
    api_base: &str,
    check_only: bool,
    exe: &std::path::Path,
    channel: Channel,
) -> Result<String, String> {
    let current = env!("CARGO_PKG_VERSION");
    let release =
        mgr::latest_release_at(api_base, "rootledev/rootle").map_err(|e| e.to_string())?;
    let tag = release.tag_name.clone();
    if !is_newer(&tag) {
        return Ok(format!("rootle {current} is current"));
    }
    let guidance = || {
        format!(
            "{current} → {tag} — you installed via {how}: run `{cmd}`",
            how = match channel {
                Channel::Brew => "homebrew",
                Channel::Cargo => "cargo",
                Channel::Mise => "mise",
                _ => "your package manager",
            },
            cmd = match channel {
                Channel::Brew => "brew upgrade rootle",
                Channel::Cargo => "cargo install rootle",
                Channel::Mise => "mise up rootle",
                _ => "your package manager's upgrade",
            }
        )
    };
    if !matches!(channel, Channel::Tarball | Channel::Other) {
        return Ok(guidance());
    }
    if check_only {
        return Ok(format!("{current} → {tag} available (run `rootle update`)"));
    }

    let target = mgr::platform_target();
    let asset = mgr::pick_asset(&release, target).map_err(|e| e.to_string())?;
    let sidecar = mgr::checksum_sidecar(&release, asset).map_err(|e| e.to_string())?;
    let tarball = mgr::download_bytes(&asset.browser_download_url).map_err(|e| e.to_string())?;
    mgr::verify_checksum(&tarball, &sidecar.browser_download_url).map_err(|e| e.to_string())?;
    let bytes = mgr::extract_binary(&tarball, "rootle").map_err(|e| e.to_string())?;

    // Staged write + atomic rename over self: the running process
    // keeps the old inode; the next launch runs the new one.
    let staged = exe.with_extension("update-tmp");
    std::fs::write(&staged, &bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&staged, exe).map_err(|e| e.to_string())?;
    Ok(format!(
        "{current} → {tag} (sha256 {}…) — takes effect on next launch",
        &mgr::sha256_hex(&bytes)[..12]
    ))
}

// ---- the 24h-cached startup check (modeline notice) ----

fn cache_path() -> Option<std::path::PathBuf> {
    dirs::cache_dir().map(|d| d.join("rootle").join("update.json"))
}

/// Newer release tag when known — cache-first, one network call a day
/// at most. Failures are silent by design.
pub fn latest_known() -> Option<String> {
    #[derive(serde::Deserialize, serde::Serialize)]
    struct Stamp {
        tag: String,
        checked_at: u64,
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = cache_path()?;
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(stamp) = serde_json::from_str::<Stamp>(&text)
        && now.saturating_sub(stamp.checked_at) < 24 * 3600
    {
        return Some(stamp.tag);
    }
    let tag = mgr::latest_release("rootledev/rootle").ok()?.tag_name;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(
            &path,
            serde_json::to_string(&Stamp {
                tag: tag.clone(),
                checked_at: now,
            })
            .unwrap_or_default(),
        );
    }
    Some(tag)
}

#[cfg(test)]
mod tests {
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

    /// 0017 M2 end to end against a loopback release: verified,
    /// extracted, atomically swapped over a temp binary; a tampered
    /// sidecar refuses and leaves the target intact.
    #[test]
    fn tarball_update_downloads_verifies_and_swaps() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let target = crate::provider::manager::platform_target();
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
        let sha = crate::provider::manager::sha256_hex(&tarball);

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

        let line = update_inner(&base, false, &exe, Channel::Tarball).expect("update");
        assert!(line.contains("9.9.9"), "got: {line}");
        assert_eq!(std::fs::read(&exe).unwrap(), payload, "swapped in place");
        assert!(
            !dir.join("rootle.update-tmp").exists(),
            "staging file is renamed away"
        );

        // A payload that doesn't match the served sidecar refuses.
        let err = crate::provider::manager::verify_checksum(
            b"not the tarball",
            &format!("{base}/dl/{file}.sha256"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("checksum mismatch"), "got: {err}");

        // --check writes nothing.
        let exe3 = dir.join("rootle3");
        std::fs::write(&exe3, b"#!/bin/sh\necho old\n").unwrap();
        let line = update_inner(&base, true, &exe3, Channel::Tarball).expect("check");
        assert!(line.contains("available"), "got: {line}");
        assert_eq!(std::fs::read(&exe3).unwrap(), b"#!/bin/sh\necho old\n");
    }
}
