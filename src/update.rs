//! App updates (plans/0017 + 0018): `rootle update` for tarball
//! installs and the 24h-cached startup check behind the modeline
//! notice.
//!
//! Integrity is the provider manager's model — the same release
//! helpers, the same mandatory `.sha256` sidecar, staged write +
//! atomic rename over self. 0018 M1: the flow drives the manager's
//! `provider::ui::Ui` stage grammar; M2: the status toast is
//! once-a-day per version and CI/dumb/non-TTY environments never
//! check; M3: a quit-time line when the on-disk binary got newer
//! under us.

use crate::provider::manager as mgr;
use std::io::IsTerminal;
use std::path::Path;

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

/// `rootle update`: tarball installs self-update (progress on stderr
/// via the manager's Ui); every other channel gets its own command.
/// `Ok(Some(line))` is for the caller to print (stdout); `Ok(None)`
/// means the flow already rendered everything. `ROOTLE_UPDATE_API`
/// points the check at a loopback host (tests, PTY evidence runs).
pub fn update(check_only: bool) -> Result<Option<String>, String> {
    let api =
        std::env::var("ROOTLE_UPDATE_API").unwrap_or_else(|_| "https://api.github.com".to_string());
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let ui = crate::provider::ui::Ui::new();
    update_inner(&api, check_only, &exe, channel(), &ui)
}

/// The flow, with the API base, target exe, and Ui swapped in tests.
fn update_inner(
    api_base: &str,
    check_only: bool,
    exe: &std::path::Path,
    channel: Channel,
    ui: &crate::provider::ui::Ui,
) -> Result<Option<String>, String> {
    let current = env!("CARGO_PKG_VERSION");
    let release =
        mgr::latest_release_at(api_base, "rootledev/rootle").map_err(|e| e.to_string())?;
    let tag = release.tag_name.clone();
    if !is_newer(&tag) {
        return Ok(Some(format!("rootle {current} is current")));
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
        return Ok(Some(guidance()));
    }
    if check_only {
        return Ok(format!("{current} → {tag} available (run `rootle update`)").into());
    }

    // 0018 M1: the manager's stage grammar, step for step.
    let timer = crate::provider::ui::Timer::start();
    ui.heading("Updating rootle");
    ui.done("Resolved", &tag);
    let target = mgr::platform_target();
    let asset = mgr::pick_asset(&release, target).map_err(|e| e.to_string())?;
    let sidecar = mgr::checksum_sidecar(&release, asset).map_err(|e| e.to_string())?;
    let spinner = ui.spinner(&format!("Downloading {}", asset.name));
    let tarball = mgr::download_bytes(&asset.browser_download_url).map_err(|e| e.to_string())?;
    drop(spinner);
    ui.step("Verifying", "sha256 checksum");
    mgr::verify_checksum(&tarball, &sidecar.browser_download_url).map_err(|e| e.to_string())?;
    ui.done("Verified", "sha256 ok");
    ui.step("Extracting", "rootle");
    let bytes = mgr::extract_binary(&tarball, "rootle").map_err(|e| e.to_string())?;
    ui.done("Extracted", "rootle");

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
    ui.done("Swapped", &exe.display().to_string());
    ui.summary(
        "Updated",
        &format!("{current} → {}", tag.trim_start_matches('v')),
        "",
        timer.elapsed(),
    );
    // keepachangelog anchor: `## [0.9.0]` → `#090`.
    let anchor: String = tag
        .trim_start_matches('v')
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    ui.note(&format!(
        "takes effect on next launch · what's new: rootle.dev/CHANGELOG.md#{anchor}"
    ));
    Ok(None)
}

// ---- the 24h-cached startup check (modeline notice) ----

const DAY: u64 = 24 * 3600;

/// The cache stamp: what the last check saw, when, and when the
/// status toast last nagged about it (0018 M2).
#[derive(serde::Deserialize, serde::Serialize)]
struct Stamp {
    tag: String,
    checked_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shown_at: Option<u64>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> Option<std::path::PathBuf> {
    dirs::cache_dir().map(|d| d.join("rootle").join("update.json"))
}

fn read_stamp(path: &Path) -> Option<Stamp> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Stamp>(&text).ok())
}

/// Newer release tag when known — cache-first, one network call a day
/// at most. Failures are silent by design.
pub fn latest_known() -> Option<String> {
    let now = unix_now();
    let path = cache_path()?;
    let prior = read_stamp(&path);
    if let Some(stamp) = &prior
        && now.saturating_sub(stamp.checked_at) < DAY
    {
        return Some(stamp.tag.clone());
    }
    let tag = mgr::latest_release("rootledev/rootle").ok()?.tag_name;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(
            &path,
            serde_json::to_string(&Stamp {
                shown_at: prior.as_ref().and_then(|p| p.shown_at),
                tag: tag.clone(),
                checked_at: now,
            })
            .unwrap_or_default(),
        );
    }
    Some(tag)
}

/// Consume the once-a-day toast quota for `tag` (0018 M2): true — and
/// stamps `shown_at` — the first time a version is seen inside a 24h
/// window; false while the window holds. The `↑` chip is unaffected;
/// this only gates the status-line nag. Worker-side, never on the UI
/// thread.
pub fn take_toast(tag: &str) -> bool {
    match cache_path() {
        Some(path) => take_toast_at(&path, tag, unix_now()),
        None => true,
    }
}

fn take_toast_at(path: &Path, tag: &str, now: u64) -> bool {
    let prior = read_stamp(path);
    let due = match &prior {
        Some(s) if s.tag == tag => !s.shown_at.is_some_and(|at| now.saturating_sub(at) < DAY),
        _ => true,
    };
    if due && let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(
            path,
            serde_json::to_string(&Stamp {
                tag: tag.to_string(),
                checked_at: prior.as_ref().map(|s| s.checked_at).unwrap_or(now),
                shown_at: Some(now),
            })
            .unwrap_or_default(),
        );
    }
    due
}

/// The notice's environment gates (0018 M2, update-informer's rules):
/// CI, dumb terminals, and non-interactive stdout never even check.
pub fn check_allowed() -> bool {
    check_allowed_from(
        std::env::var("CI").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
        std::io::stdout().is_terminal(),
    )
}

fn check_allowed_from(ci: Option<&str>, term: Option<&str>, stdout_tty: bool) -> bool {
    let ci = !matches!(ci, None | Some("") | Some("0") | Some("false"));
    !ci && term != Some("dumb") && stdout_tty
}

// ---- the quit-time restart trace (0018 M3) ----

/// The exit line when the on-disk binary is newer than the running
/// one — an update landed in a shell under this session.
pub fn exit_note(running: &str, disk: &str) -> Option<String> {
    let (r, d) = (parse_version(running)?, parse_version(disk)?);
    (d > r).then(|| {
        format!(
            "v{} installed — relaunch for it",
            disk.trim_start_matches('v')
        )
    })
}

/// Compare once at exit: `current_exe --version` runs the swapped-in
/// build (the path is new; this process still holds the old inode).
pub fn disk_newer_note() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let out = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let disk = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .last()?
        .to_string();
    exit_note(env!("CARGO_PKG_VERSION"), &disk)
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

        let (ui, log) = crate::provider::ui::Ui::recorder();
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
            " ▸ takes effect on next launch · what's new: rootle.dev/CHANGELOG.md#999",
            "changelog note"
        );
        assert_eq!(lines.len(), expect.len() + 2, "no stray lines: {lines:?}");

        // A payload that doesn't match the served sidecar refuses.
        let err = crate::provider::manager::verify_checksum(
            b"not the tarball",
            &format!("{base}/dl/{file}.sha256"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("checksum mismatch"), "got: {err}");

        // --check writes nothing and renders nothing.
        let exe3 = dir.join("rootle3");
        std::fs::write(&exe3, b"#!/bin/sh\necho old\n").unwrap();
        let (ui3, log3) = crate::provider::ui::Ui::recorder();
        let line = update_inner(&base, true, &exe3, Channel::Tarball, &ui3).expect("check");
        assert!(
            line.as_deref().unwrap_or_default().contains("available"),
            "got: {line:?}"
        );
        assert_eq!(std::fs::read(&exe3).unwrap(), b"#!/bin/sh\necho old\n");
        assert!(log3.lock().unwrap().is_empty(), "check renders no steps");
    }
}
