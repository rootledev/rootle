//! App updates (plans/0017 + 0018): `rootle update` for tarball
//! installs and the 24h-cached startup check behind the modeline
//! notice.
//!
//! Integrity is the provider manager's model — the same release
//! helpers, the same mandatory `.sha256` sidecar, staged write +
//! atomic rename over self. 0018 M1: the flow drives the manager's
//! `rootle_manager::progress::ProgressOutput` stage grammar; M2: the status toast is
//! once-a-day per version and CI/dumb/non-TTY environments never
//! check; M3: a quit-time line when the on-disk binary got newer
//! under us.

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

/// `rootle update`: the app half (tarball self-update with progress
/// on stderr via the manager's Ui; other channels get their command
/// on stdout), then the provider sweep (0019 M1) — every managed,
/// unpinned, releases-tracked provider refreshed and upgraded with
/// failures isolated. Any provider failure fails the command after
/// everything was attempted. `ROOTLE_UPDATE_API` points the app half
/// at a loopback host (tests, PTY evidence runs).
pub fn update(check_only: bool) -> Result<(), String> {
    let ui = rootle_manager::progress::ProgressOutput::new();
    update_application(check_only, &ui)?;
    sweep_providers(check_only, &ui)
}

/// Application-only update: never scans or changes provider installations.
pub fn self_update(check_only: bool) -> Result<(), String> {
    update_application(check_only, &rootle_manager::progress::ProgressOutput::new())
}

fn update_application(
    check_only: bool,
    ui: &rootle_manager::progress::ProgressOutput,
) -> Result<(), String> {
    let api =
        std::env::var("ROOTLE_UPDATE_API").unwrap_or_else(|_| "https://api.github.com".to_string());
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if let Some(message) = update_inner(&api, check_only, &executable, channel(), ui)? {
        println!("{message}");
    }
    Ok(())
}

/// The flow, with the API base, target exe, and Ui swapped in tests.
fn update_inner(
    api_base: &str,
    check_only: bool,
    exe: &std::path::Path,
    channel: Channel,
    ui: &rootle_manager::progress::ProgressOutput,
) -> Result<Option<String>, String> {
    rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
        serde_json::json!({"operation":"self_update", "phase":"started", "check_only":check_only,
            "channel":format!("{channel:?}")})
    });
    let current = env!("CARGO_PKG_VERSION");
    let release = rootle_manager::latest_release_at(api_base, "rootledev/rootle")
        .map_err(|e| e.to_string())?;
    let tag = release.tag_name.clone();
    if !is_newer(&tag) {
        rootle_trace::record_with(
            rootle_trace::EventKind::ExternalCommand,
            || serde_json::json!({"operation":"self_update","phase":"current","version":current,"latest":tag}),
        );
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
        rootle_trace::record_with(
            rootle_trace::EventKind::ExternalCommand,
            || serde_json::json!({"operation":"self_update","phase":"managed_channel","latest":tag}),
        );
        return Ok(Some(guidance()));
    }
    if check_only {
        rootle_trace::record_with(
            rootle_trace::EventKind::ExternalCommand,
            || serde_json::json!({"operation":"self_update","phase":"available","latest":tag}),
        );
        return Ok(format!("{current} → {tag} available (run `rootle self-update`)").into());
    }

    // 0018 M1: the manager's stage grammar, step for step.
    let timer = rootle_manager::progress::Timer::start();
    ui.heading("Updating rootle");
    ui.done("Resolved", &tag);
    let target = rootle_manager::platform_target();
    let asset = rootle_manager::pick_asset(&release, target).map_err(|e| e.to_string())?;
    let sidecar = rootle_manager::checksum_sidecar(&release, asset).map_err(|e| e.to_string())?;
    let spinner = ui.spinner(&format!("Downloading {}", asset.name));
    let tarball =
        rootle_manager::download_bytes(&asset.browser_download_url).map_err(|e| e.to_string())?;
    drop(spinner);
    ui.step("Verifying", "sha256 checksum");
    rootle_manager::verify_checksum(&tarball, &sidecar.browser_download_url)
        .map_err(|e| e.to_string())?;
    ui.done("Verified", "sha256 ok");
    ui.step("Extracting", "rootle");
    let bytes = rootle_manager::extract_binary(&tarball, "rootle").map_err(|e| e.to_string())?;
    ui.done("Extracted", "rootle");

    // Staged write + atomic rename over self: the running process
    // keeps the old inode; the next launch runs the new one.
    let staged = exe.with_extension("update-tmp");
    rootle_trace::record_with(
        rootle_trace::EventKind::ExternalCommand,
        || serde_json::json!({"operation":"self_update","phase":"staging","bytes":bytes.len()}),
    );
    std::fs::write(&staged, &bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&staged, exe).map_err(|e| e.to_string())?;
    rootle_trace::record_with(
        rootle_trace::EventKind::ExternalCommand,
        || serde_json::json!({"operation":"self_update","phase":"swapped","bytes":bytes.len(),"version":tag}),
    );
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
        "takes effect on next launch · what's new: rootle.dev/changelog#{anchor}"
    ));
    Ok(None)
}

// ---- the provider sweep (0019 M1) ----

/// The provider half of `rootle update`: every managed, unpinned,
/// releases-tracked provider refreshed and upgraded, failures
/// isolated per provider. No receipts on this machine (or no data
/// dir at all): the section is skipped silently.
fn sweep_providers(
    check_only: bool,
    ui: &rootle_manager::progress::ProgressOutput,
) -> Result<(), String> {
    let manager = match rootle_manager::Manager::new() {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if manager.receipts().is_empty() {
        return Ok(());
    }
    let timer = rootle_manager::progress::Timer::start();
    ui.heading("Updating providers");
    let outcomes = manager.sweep(check_only, ui);
    rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
        serde_json::json!({"operation":"provider_sweep","phase":"finished","check_only":check_only,
            "providers":outcomes.len(),
            "failed":outcomes.iter().filter(|outcome|matches!(outcome,rootle_manager::SweepOutcome::Failed{..})).count()})
    });
    render_sweep(&outcomes, ui, timer.elapsed());
    let failed: Vec<&str> = outcomes
        .iter()
        .filter_map(|o| match o {
            rootle_manager::SweepOutcome::Failed { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("provider sweep failed: {}", failed.join(", ")))
    }
}

/// Outcome rows + the counts summary. `Upgraded` carries no row —
/// `install_inner` already rendered that provider's full stage block
/// through the same Ui.
fn render_sweep(
    outcomes: &[rootle_manager::SweepOutcome],
    ui: &rootle_manager::progress::ProgressOutput,
    elapsed: std::time::Duration,
) {
    let (mut upgraded, mut current, mut pinned, mut untracked, mut failed) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for outcome in outcomes {
        match outcome {
            rootle_manager::SweepOutcome::Upgraded { .. } => upgraded += 1,
            rootle_manager::SweepOutcome::Stale { name, from, to } => {
                ui.update_row("→", name, &format!("{from} → {to} (run `rootle update`)"));
            }
            rootle_manager::SweepOutcome::Current { name, tag } => {
                current += 1;
                ui.update_row("·", name, &format!("{tag} current"));
            }
            rootle_manager::SweepOutcome::Pinned { name, tag } => {
                pinned += 1;
                ui.update_row("📌", name, &format!("{tag} pinned — skipped"));
            }
            rootle_manager::SweepOutcome::Untracked { name, source } => {
                untracked += 1;
                ui.update_row("·", name, &format!("{source} install-and-pin — untouched"));
            }
            rootle_manager::SweepOutcome::Failed { name, error } => {
                failed += 1;
                ui.update_row("✗", name, error);
            }
        }
    }
    let mut detail = format!("{upgraded} upgraded · {current} current · {pinned} pinned");
    if untracked > 0 {
        detail.push_str(&format!(" · {untracked} install-and-pin"));
    }
    if failed > 0 {
        detail.push_str(&format!(" · {failed} failed"));
    }
    ui.summary("Swept", "providers", &detail, elapsed);
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
    rootle_provider::paths::cache_dir().map(|d| d.join("rootle").join("update.json"))
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
    let Some(path) = cache_path() else {
        rootle_trace::record_with(
            rootle_trace::EventKind::Cache,
            || serde_json::json!({"resource":"update_notice","outcome":"no_cache_directory"}),
        );
        return None;
    };
    let prior = read_stamp(&path);
    rootle_trace::record_with(rootle_trace::EventKind::Cache, || {
        serde_json::json!({"resource":"update_notice","operation":"read","hit":prior.is_some(),
            "fresh":prior.as_ref().is_some_and(|stamp|now.saturating_sub(stamp.checked_at)<DAY)})
    });
    if let Some(stamp) = &prior
        && now.saturating_sub(stamp.checked_at) < DAY
    {
        return Some(stamp.tag.clone());
    }
    let tag = match rootle_manager::latest_release("rootledev/rootle") {
        Ok(release) => release.tag_name,
        Err(_) => {
            rootle_trace::record_with(
                rootle_trace::EventKind::Error,
                || serde_json::json!({"operation":"update_probe","outcome":"unavailable"}),
            );
            return None;
        }
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let written = std::fs::write(
            &path,
            serde_json::to_string(&Stamp {
                shown_at: prior.as_ref().and_then(|p| p.shown_at),
                tag: tag.clone(),
                checked_at: now,
            })
            .unwrap_or_default(),
        );
        rootle_trace::record_with(rootle_trace::EventKind::Cache, || {
            serde_json::json!({"resource":"update_notice","operation":"write","success":written.is_ok(),
                "error_kind":written.as_ref().err().map(|error|format!("{:?}",error.kind()))})
        });
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
    rootle_trace::record_with(
        rootle_trace::EventKind::Cache,
        || serde_json::json!({"resource":"update_notice","operation":"toast","due":due}),
    );
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
mod tests;
