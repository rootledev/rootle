//! GitHub release assets: the API calls, the platform matrix, tarball
//! extraction, and the mandatory sha256 sidecar verification.

use super::{ManagerError, Result};
use serde::Deserialize;
use std::time::Duration;

/// The GitHub release this rootle downloads for (the 4-target matrix).
pub(super) fn platform_target() -> &'static str {
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
pub(super) struct Asset {
    pub(super) name: String,
    pub(super) browser_download_url: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Release {
    pub(super) tag_name: String,
    pub(super) assets: Vec<Asset>,
}

fn http() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(format!("rootle/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

pub(super) fn latest_release(repo: &str) -> Result<Release> {
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

pub(super) fn release_by_tag(repo: &str, tag: &str) -> Result<Release> {
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
pub(super) fn pick_asset<'a>(release: &'a Release, target: &str) -> Result<&'a Asset> {
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

/// The `.sha256` sidecar for `asset` — no sidecar, no install.
pub(super) fn checksum_sidecar<'a>(release: &'a Release, asset: &Asset) -> Result<&'a Asset> {
    release
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sha256", asset.name))
        .ok_or_else(|| {
            ManagerError::User(format!(
                "release {} has no checksum sidecar for {} — refusing to install \
                 without verification",
                release.tag_name, asset.name
            ))
        })
}

pub(super) fn download_bytes(url: &str) -> Result<Vec<u8>> {
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
pub(super) fn extract_binary(tarball: &[u8], binary_name: &str) -> Result<Vec<u8>> {
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

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verify against the `.sha256` sidecar asset — mandatory (krew rule:
/// a missing checksum is a failed install, not a warning).
pub(super) fn verify_checksum(tarball: &[u8], sidecar_url: &str) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
