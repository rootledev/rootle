//! Provider manager (plans/0010): install, list, update, upgrade,
//! pin, remove, use — for stdio provider binaries distributed as
//! GitHub release tarballs (the 4-target matrix rootle and its
//! providers ship: linux+macOS × x86_64+aarch64).
//!
//! Layout (XDG tiers, one purpose each — see the plan's survey):
//!
//! ```text
//! ~/.local/share/rootle/providers/<name>/<version>/rootle-<name>  binary
//! ~/.local/share/rootle/providers/<name>/current -> <version>/    pointer
//! ~/.local/state/rootle/providers/<name>.toml                    receipt
//! ```
//!
//! Atomicity (krew model): download to temp staging → verify sha256
//! (mandatory) → extract into the versioned dir → write the receipt
//! LAST → atomically re-point `current`. A failed step leaves the
//! previous version intact. The running TUI is never affected —
//! provider children die with the app, the next spawn picks up the
//! new `current`.
//!
//! Config declares, the store installs (mise pattern): `use` writes
//! the existing `[provider]` block; the runtime `build()` is
//! untouched and hand-edited configs keep working.

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Where installed binaries live (XDG data).
fn store_root() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("rootle").join("providers"))
}

/// Where receipts live (XDG state).
fn state_root() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .map(|d| d.join("rootle").join("providers"))
}

/// The receipt: provenance recorded beside the binary so list/upgrade
/// read local state, never the network (gh manifest.yml pattern).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub name: String,
    /// "owner/repo"
    pub source: String,
    pub tag: String,
    pub sha256: String,
    pub pinned: bool,
    #[serde(default)]
    pub installed_at: Option<String>,
    /// Latest tag known from `update` (krew's non-mutating refresh).
    #[serde(default)]
    pub latest_tag: Option<String>,
}
type Result<T, E = ManagerError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("{0}")]
    User(String),
    #[error("network: {0}")]
    Network(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// A resolved install reference (the grammar from the plan):
/// `gitlab` | `owner/repo` | `https://github.com/owner/repo`, each
/// optionally `@tag`-pinned.
#[derive(Debug, Clone, PartialEq)]
pub struct Ref {
    /// owner/repo
    pub repo: String,
    /// The bare name (repo stem minus the rootle- prefix, or the stem
    /// itself when unprefixed).
    pub name: String,
    pub tag: Option<String>,
}

impl Ref {
    pub fn parse(input: &str) -> Result<Ref> {
        let input = input.trim();
        if input.is_empty() {
            return Err(ManagerError::User("empty provider reference".into()));
        }
        let (path, tag) = match input.split_once('@') {
            Some((p, t)) => (p, Some(t.to_string())),
            None => (input, None),
        };
        // Full URL → owner/repo
        let repo = if let Some(rest) = path
            .strip_prefix("https://github.com/")
            .or_else(|| path.strip_prefix("http://github.com/"))
        {
            rest.trim_end_matches('/')
                .trim_end_matches(".git")
                .to_string()
        } else {
            path.to_string()
        };
        let has_slash = repo.contains('/');
        let short = if has_slash {
            let stem = repo.rsplit('/').next().unwrap_or(&repo).to_string();
            stem.strip_prefix("rootle-").unwrap_or(&stem).to_string()
        } else {
            repo.clone()
        };
        let repo = if has_slash {
            repo
        } else {
            // Bare name → the rootle-<name> convention (gh's gh- rule).
            format!("rootledev/rootle-{repo}")
        };
        let Some((owner, name)) = repo.split_once('/') else {
            return Err(ManagerError::User(format!(
                "provider reference {input:?} must be owner/repo, a GitHub URL, or a bare name"
            )));
        };
        if owner.is_empty() || name.is_empty() {
            return Err(ManagerError::User(format!("malformed reference {input:?}")));
        }
        Ok(Ref {
            repo,
            name: short,
            tag,
        })
    }
}

/// The GitHub release this rootle downloads for (the 4-target matrix).
fn platform_target() -> &'static str {
    if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-musl"
    } else if cfg!(target_os = "linux") {
        "x86_64-unknown-linux-musl"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else if cfg!(target_os = "macos") {
        "x86_64-apple-darwin"
    } else {
        "x86_64-unknown-linux-musl"
    }
}

/// One release asset, as the GitHub API reports it.
#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

fn http() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(format!("rootle/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

fn latest_release(repo: &str) -> Result<Release> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    http()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| ManagerError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| ManagerError::Network(format!("github api: {e}")))?
        .json()
        .map_err(|e| ManagerError::Network(format!("github api decode: {e}")))
}

fn release_by_tag(repo: &str, tag: &str) -> Result<Release> {
    let url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");
    http()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| ManagerError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| ManagerError::Network(format!("github api: {e}")))?
        .json()
        .map_err(|e| ManagerError::Network(format!("github api decode: {e}")))
}

/// Pick the asset for this platform: `<anything>-<target>.tar.gz`,
/// suffix-matched (gh extension's model — tolerate version prefixes
/// and name variants).
fn pick_asset<'a>(release: &'a Release, target: &str) -> Result<&'a Asset> {
    release
        .assets
        .iter()
        .find(|a| a.name.ends_with(&format!("-{target}.tar.gz")))
        .ok_or_else(|| {
            ManagerError::User(format!(
                "release {} has no {target} tarball (assets: {})",
                release.tag_name,
                release
                    .assets
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

fn download_bytes(url: &str) -> Result<Vec<u8>> {
    http()
        .get(url)
        .send()
        .map_err(|e| ManagerError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| ManagerError::Network(format!("download: {e}")))?
        .bytes()
        .map(|b| b.to_vec())
        .map_err(|e| ManagerError::Network(e.to_string()))
}

/// Extract the binary from the tarball: find the single executable
/// file, return its bytes (our tarballs contain `<dir>/<binary>`).
fn extract_binary(tarball: &[u8], binary_name: &str) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use tar::Archive;
    let decoder = GzDecoder::new(tarball);
    let mut archive = Archive::new(decoder);
    for entry in archive
        .entries()
        .map_err(|e| ManagerError::User(format!("tarball: {e}")))?
    {
        let mut entry = entry.map_err(|e| ManagerError::User(format!("tarball: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| ManagerError::User(format!("tarball: {e}")))?
            .to_string_lossy()
            .into_owned();
        if path.ends_with(binary_name) && entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            use std::io::Read;
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| ManagerError::User(format!("tarball: {e}")))?;
            return Ok(bytes);
        }
    }
    Err(ManagerError::User(format!(
        "no {binary_name} inside the tarball"
    )))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verify against the `.sha256` sidecar asset — mandatory (krew rule:
/// a missing checksum is a failed install, not a warning).
fn verify_checksum(tarball: &[u8], sidecar_url: &str) -> Result<()> {
    let sidecar = download_bytes(sidecar_url)?;
    let text = String::from_utf8_lossy(&sidecar);
    let expected = text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let got = sha256_hex(tarball);
    if got != expected {
        return Err(ManagerError::User(format!(
            "checksum mismatch: expected {expected}, got {got}"
        )));
    }
    Ok(())
}

pub struct Manager {
    store: PathBuf,
    state: PathBuf,
}

/// An installed provider, as `list` reports.
#[derive(Debug, Clone)]
pub struct Installed {
    pub receipt: Receipt,
    pub active: bool,
    pub current: Option<String>,
}

impl Manager {
    pub fn new() -> Result<Manager> {
        Ok(Manager {
            store: store_root().ok_or_else(|| ManagerError::User("no data dir".into()))?,
            state: state_root().ok_or_else(|| ManagerError::User("no state dir".into()))?,
        })
    }

    fn receipt_path(&self, name: &str) -> PathBuf {
        self.state.join(format!("{name}.toml"))
    }

    fn version_dir(&self, name: &str, tag: &str) -> PathBuf {
        self.store.join(name).join(tag)
    }

    fn current_link(&self, name: &str) -> PathBuf {
        self.store.join(name).join("current")
    }

    /// The binary path the `current` symlink resolves to.
    pub fn current_binary(&self, name: &str) -> Option<PathBuf> {
        let link = self.current_link(name);
        let resolved = std::fs::read_link(&link).ok()?;
        let dir = link.parent()?.join(resolved);
        let stem = format!("rootle-{name}");
        let bin = dir.join(&stem);
        bin.is_file().then_some(bin)
    }

    pub fn receipt(&self, name: &str) -> Option<Receipt> {
        let text = std::fs::read_to_string(self.receipt_path(name)).ok()?;
        toml::from_str(&text).ok()
    }

    pub fn receipts(&self) -> Vec<Receipt> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.state) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "toml")
                    && let Ok(text) = std::fs::read_to_string(&path)
                    && let Ok(r) = toml::from_str::<Receipt>(&text)
                {
                    out.push(r);
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Install (or upgrade to a specific tag). The krew atomicity
    /// sequence: staging → verify → extract → receipt LAST → swap.
    pub fn install(&self, r: &Ref, force: bool) -> Result<Receipt> {
        if let Some(existing) = self.receipt(&r.name)
            && existing.tag == r.tag.clone().unwrap_or_default()
            && !force
            && existing.source == r.repo
        {
            return Err(ManagerError::User(format!(
                "{} {} is already installed (use --force to reinstall)",
                r.name, existing.tag
            )));
        }
        println!("resolving {repo}…", repo = r.repo);
        let release = match &r.tag {
            Some(tag) => release_by_tag(&r.repo, tag)?,
            None => latest_release(&r.repo)?,
        };
        let target = platform_target();
        let asset = pick_asset(&release, target)?;
        let sidecar = release
            .assets
            .iter()
            .find(|a| a.name == format!("{}.sha256", asset.name))
            .ok_or_else(|| {
                ManagerError::User(format!(
                    "release {} has no checksum sidecar for {} — refusing to install \
                     without verification",
                    release.tag_name, asset.name
                ))
            })?;

        println!("downloading {}…", asset.name);
        let tarball = download_bytes(&asset.browser_download_url)?;
        verify_checksum(&tarball, &sidecar.browser_download_url)?;
        println!("checksum ok");

        let binary_name = format!("rootle-{}", r.name);
        let bytes = extract_binary(&tarball, &binary_name)?;

        // Versioned dir + receipt LAST + pointer swap.
        let vdir = self.version_dir(&r.name, &release.tag_name);
        std::fs::create_dir_all(&vdir)?;
        let bin = vdir.join(&binary_name);
        std::fs::write(&bin, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }

        let receipt = Receipt {
            name: r.name.clone(),
            source: r.repo.clone(),
            tag: release.tag_name.clone(),
            sha256: sha256_hex(&tarball),
            pinned: r.tag.is_some(),
            installed_at: now_iso(),
            latest_tag: Some(release.tag_name.clone()),
        };
        self.write_receipt(&receipt)?;
        self.point_current(&r.name, &release.tag_name)?;

        println!("{} {} installed", r.name, release.tag_name);
        println!(
            "you are trusting {repo} — run `rootle provider use {name}` to activate",
            repo = r.repo,
            name = r.name
        );
        Ok(receipt)
    }

    /// Local dev install: symlink a binary, no network (gh's `gh
    /// extension install .` model).
    pub fn install_path(&self, name: &str, path: &Path) -> Result<Receipt> {
        if !path.is_file() {
            return Err(ManagerError::User(format!(
                "{} is not a file",
                path.display()
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
        }
        let vdir = self.version_dir(name, "local");
        std::fs::create_dir_all(&vdir)?;
        let bin = vdir.join(format!("rootle-{name}"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(path, &bin)
            .or_else(|_| std::fs::copy(path, &bin).map(|_| ()))
            .map_err(ManagerError::Io)?;
        #[cfg(not(unix))]
        std::fs::copy(path, &bin)?;
        let receipt = Receipt {
            name: name.into(),
            source: path.display().to_string(),
            tag: "local".into(),
            sha256: String::new(),
            pinned: true, // local installs never auto-upgrade
            installed_at: now_iso(),
            latest_tag: None,
        };
        self.write_receipt(&receipt)?;
        self.point_current(name, "local")?;
        println!("{name} installed (local: {path})", path = path.display());
        Ok(receipt)
    }

    /// krew's non-mutating `update`: refresh the latest-known tag into
    /// receipts. Returns (name, current, latest) for the ones that are
    /// stale.
    pub fn update(&self, name: Option<&str>) -> Result<Vec<(String, String, String)>> {
        let mut stale = Vec::new();
        for receipt in self.receipts() {
            if let Some(want) = name
                && receipt.name != want
            {
                continue;
            }
            if receipt.pinned {
                continue;
            }
            match latest_release(&receipt.source) {
                Ok(rel) => {
                    let latest = rel.tag_name;
                    if latest != receipt.tag {
                        stale.push((receipt.name.clone(), receipt.tag.clone(), latest.clone()));
                    }
                    let mut updated = receipt.clone();
                    updated.latest_tag = Some(latest);
                    self.write_receipt(&updated)?;
                }
                Err(e) => eprintln!("{}: {e}", receipt.name),
            }
        }
        Ok(stale)
    }

    /// `upgrade`: swap the binary for the latest-known (or fresh
    /// latest) tag. Pinned skips unless force.
    pub fn upgrade(&self, name: Option<&str>, dry_run: bool, force: bool) -> Result<()> {
        for receipt in self.receipts() {
            if let Some(want) = name
                && receipt.name != want
            {
                continue;
            }
            if receipt.pinned && !force {
                println!("{}: pinned at {} (use --force)", receipt.name, receipt.tag);
                continue;
            }
            let latest = match &receipt.latest_tag {
                Some(t) => t.clone(),
                None => latest_release(&receipt.source)?.tag_name,
            };
            if latest == receipt.tag {
                println!("{}: {} is current", receipt.name, receipt.tag);
                continue;
            }
            if dry_run {
                println!("{}: {} → {}", receipt.name, receipt.tag, latest);
                continue;
            }
            let r = Ref {
                repo: receipt.source.clone(),
                name: receipt.name.clone(),
                tag: None,
            };
            // Force through the already-installed check.
            self.install(&r, true)?;
        }
        Ok(())
    }

    pub fn pin(&self, name: &str, tag: Option<String>) -> Result<()> {
        let mut receipt = self
            .receipt(name)
            .ok_or_else(|| ManagerError::User(format!("{name} is not installed")))?;
        if let Some(tag) = tag {
            receipt.tag = tag;
        }
        receipt.pinned = true;
        self.write_receipt(&receipt)
    }

    pub fn unpin(&self, name: &str) -> Result<()> {
        let mut receipt = self
            .receipt(name)
            .ok_or_else(|| ManagerError::User(format!("{name} is not installed")))?;
        receipt.pinned = false;
        self.write_receipt(&receipt)
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        let path = self.receipt_path(name);
        if !path.exists() {
            return Err(ManagerError::User(format!("{name} is not installed")));
        }
        let dir = self.store.join(name);
        let _ = std::fs::remove_dir_all(dir);
        std::fs::remove_file(path)?;
        println!("{name} removed");
        Ok(())
    }

    /// mise's `use`: write the active provider into config.toml.
    pub fn activate(&self, name: &str, extra_argv: &[String]) -> Result<()> {
        let binary = self
            .current_binary(name)
            .ok_or_else(|| ManagerError::User(format!("{name} is not installed")))?;
        let mut config = Config::load();
        let mut command = vec![binary.display().to_string()];
        command.extend(extra_argv.iter().cloned());
        config.provider.kind = "stdio".into();
        config.provider.command = command;
        config
            .save()
            .map_err(|e| ManagerError::User(format!("save config: {e}")))?;
        println!("{name} is now the active provider (restart rootle to apply)");
        Ok(())
    }

    /// Everything `list` shows, including the ACTIVE row.
    pub fn list(&self) -> Vec<Installed> {
        let config = Config::load();
        let active_command = if config.provider.kind == "stdio" {
            config.provider.command.first().cloned()
        } else {
            None
        };
        self.receipts()
            .into_iter()
            .map(|receipt| {
                let current = std::fs::read_link(self.current_link(&receipt.name))
                    .ok()
                    .and_then(|p| {
                        p.to_str()
                            .and_then(|s| s.strip_suffix('/'))
                            .map(str::to_string)
                            .or_else(|| p.to_str().map(str::to_string))
                    });
                let active = active_command.as_ref().is_some_and(|cmd| {
                    cmd.contains(&format!("providers/{}/current/", receipt.name))
                });
                Installed {
                    receipt,
                    active,
                    current,
                }
            })
            .collect()
    }

    fn write_receipt(&self, receipt: &Receipt) -> Result<()> {
        std::fs::create_dir_all(&self.state)?;
        let path = self.receipt_path(&receipt.name);
        let tmp = path.with_extension("toml.tmp");
        let text =
            toml::to_string_pretty(receipt).map_err(|e| ManagerError::User(e.to_string()))?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path).map_err(ManagerError::Io)?;
        Ok(())
    }

    fn point_current(&self, name: &str, tag: &str) -> Result<()> {
        let link = self.current_link(name);
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(format!("{tag}/"), &link).map_err(ManagerError::Io)?;
        Ok(())
    }
}

fn now_iso() -> Option<String> {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs();
    // days since epoch → y/m/d (civil_from_days, Howard Hinnant).
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    let (h, min, s) = ((secs % 86_400) / 3600, (secs % 3600) / 60, secs % 60);
    Some(format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z"))
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_grammar() {
        // Bare name → the convention repo.
        assert_eq!(
            Ref::parse("gitlab").unwrap(),
            Ref {
                repo: "rootledev/rootle-gitlab".into(),
                name: "gitlab".into(),
                tag: None
            }
        );
        // owner/repo keeps its name; prefix stripped for the short name.
        assert_eq!(
            Ref::parse("rootledev/rootle-gitlab").unwrap(),
            Ref {
                repo: "rootledev/rootle-gitlab".into(),
                name: "gitlab".into(),
                tag: None
            }
        );
        // Full URL.
        assert_eq!(
            Ref::parse("https://github.com/rootledev/rootle-gitlab").unwrap(),
            Ref {
                repo: "rootledev/rootle-gitlab".into(),
                name: "gitlab".into(),
                tag: None
            }
        );
        // Tag pin.
        assert_eq!(
            Ref::parse("rootledev/rootle-gitlab@v0.1.0").unwrap().tag,
            Some("v0.1.0".into())
        );
        // Bare name with tag.
        assert_eq!(
            Ref::parse("gitlab@v0.2.0").unwrap().tag,
            Some("v0.2.0".into())
        );
        // Malformed.
        assert!(Ref::parse("").is_err());
        assert!(Ref::parse("justtext/").is_err());
        // Unprefixed repo: stem is the name.
        assert_eq!(Ref::parse("someone/myprovider").unwrap().name, "myprovider");
    }

    #[test]
    fn asset_picking_matches_the_matrix() {
        let release = Release {
            tag_name: "v0.1.0".into(),
            assets: vec![
                Asset {
                    name: "rootle-gitlab-0.1.0-x86_64-unknown-linux-musl.tar.gz".into(),
                    browser_download_url: "u1".into(),
                },
                Asset {
                    name: "rootle-gitlab-0.1.0-aarch64-unknown-linux-musl.tar.gz".into(),
                    browser_download_url: "u2".into(),
                },
                Asset {
                    name: "rootle-gitlab-0.1.0-x86_64-apple-darwin.tar.gz".into(),
                    browser_download_url: "u3".into(),
                },
                Asset {
                    name: "rootle-gitlab-0.1.0-aarch64-apple-darwin.tar.gz".into(),
                    browser_download_url: "u4".into(),
                },
            ],
        };
        assert_eq!(
            pick_asset(&release, "aarch64-apple-darwin")
                .unwrap()
                .browser_download_url,
            "u4"
        );
        assert!(pick_asset(&release, "i686-unknown-linux-gnu").is_err());
    }

    #[test]
    fn iso_date_math() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20690), (2026, 8, 25));
    }
}
