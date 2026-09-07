//! GitHub release assets: the API calls, the platform matrix, tarball
//! extraction, and the mandatory sha256 sidecar verification.

use super::{ManagerError, Result};
use serde::Deserialize;
use std::time::Duration;

/// The GitHub release this rootle downloads for (the 4-target matrix).
pub fn platform_target() -> &'static str {
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
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub(crate) assets: Vec<Asset>,
}

fn http() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(format!("rootle/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

/// One request/body boundary for release metadata and artifacts. Payloads stay
/// in memory for their consumer; diagnostics retain only safe endpoint metadata.
fn fetch<T>(
    url: &str,
    release_metadata: bool,
    decode: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<T> {
    let request_id = rootle_trace::operation_id();
    let started = rootle_trace::enabled().then(std::time::Instant::now);
    rootle_trace::record_with(rootle_trace::EventKind::HttpRequest, || {
        let endpoint = reqwest::Url::parse(url).ok();
        serde_json::json!({
            "client":"manager", "request_id":request_id, "method":"GET",
            "resource":if release_metadata { "release_metadata" } else { "artifact" },
            "scheme":endpoint.as_ref().map(reqwest::Url::scheme),
            "host":endpoint.as_ref().and_then(reqwest::Url::host_str),
            "port":endpoint.as_ref().and_then(reqwest::Url::port),
        })
    });
    let mut request = http().get(url);
    if release_metadata {
        request = request.header("Accept", "application/vnd.github+json");
    }
    let response = request.send().map_err(|error| {
        record_http(request_id, started, None, None, http_error_kind(&error));
        ManagerError::Network(error.to_string())
    })?;
    let status = response.status().as_u16();
    let response = response.error_for_status().map_err(|error| {
        record_http(request_id, started, Some(status), None, "http_error");
        let source = if release_metadata {
            "github api"
        } else {
            "download"
        };
        ManagerError::Network(format!("{source}: {error}"))
    })?;
    let bytes = response.bytes().map_err(|error| {
        record_http(
            request_id,
            started,
            Some(status),
            None,
            http_error_kind(&error),
        );
        ManagerError::Network(error.to_string())
    })?;
    let result = decode(&bytes);
    record_http(
        request_id,
        started,
        Some(status),
        Some(bytes.len()),
        if result.is_ok() {
            "success"
        } else {
            "decode_error"
        },
    );
    result
}

fn record_http(
    request_id: Option<rootle_trace::OperationId>,
    started: Option<std::time::Instant>,
    status: Option<u16>,
    bytes: Option<usize>,
    outcome: &'static str,
) {
    rootle_trace::record_with(rootle_trace::EventKind::HttpResponse, || {
        serde_json::json!({"client":"manager", "request_id":request_id, "status":status,
            "body_bytes":bytes, "outcome":outcome,
            "duration_us":started.map(|started|started.elapsed().as_micros())})
    });
}

fn http_error_kind(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_builder() {
        "request"
    } else if error.is_body() {
        "body"
    } else if error.is_decode() {
        "decode"
    } else {
        "transport"
    }
}

pub fn latest_release(repo: &str) -> Result<Release> {
    latest_release_at("https://api.github.com", repo)
}

/// Tests point this at a loopback host.
pub fn latest_release_at(api: &str, repo: &str) -> Result<Release> {
    let url = format!("{api}/repos/{repo}/releases/latest");
    fetch(&url, true, |bytes| {
        serde_json::from_slice(bytes)
            .map_err(|error| ManagerError::Network(format!("github api decode: {error}")))
    })
}

/// Tests point this at a loopback host.
pub fn release_by_tag_at(api: &str, repo: &str, tag: &str) -> Result<Release> {
    let url = format!("{api}/repos/{repo}/releases/tags/{tag}");
    fetch(&url, true, |bytes| {
        serde_json::from_slice(bytes)
            .map_err(|error| ManagerError::Network(format!("github api decode: {error}")))
    })
}

/// Pick the asset for this platform: `<anything>-<target>.tar.gz`,
/// suffix-matched (gh extension's model — tolerate version prefixes
/// and name variants).
pub fn pick_asset<'a>(release: &'a Release, target: &str) -> Result<&'a Asset> {
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
pub fn checksum_sidecar<'a>(release: &'a Release, asset: &Asset) -> Result<&'a Asset> {
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

pub fn download_bytes(url: &str) -> Result<Vec<u8>> {
    fetch(url, false, |bytes| Ok(bytes.to_vec()))
}

/// Extract the binary from the tarball: find the single executable
/// file, return its bytes (our tarballs contain `<dir>/<binary>`).
pub fn extract_binary(tarball: &[u8], binary_name: &str) -> Result<Vec<u8>> {
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

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verify against the `.sha256` sidecar asset — mandatory (krew rule:
/// a missing checksum is a failed install, not a warning).
pub fn verify_checksum(tarball: &[u8], sidecar_url: &str) -> Result<()> {
    let sidecar = download_bytes(sidecar_url)?;
    let text = String::from_utf8_lossy(&sidecar);
    let expected = text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let got = sha256_hex(tarball);
    rootle_trace::record_with(
        rootle_trace::EventKind::ExternalCommand,
        || serde_json::json!({"operation":"checksum_verify", "bytes":tarball.len(), "matched":got == expected}),
    );
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
