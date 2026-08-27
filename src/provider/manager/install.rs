//! The network flows: release install (krew atomicity), local symlink
//! install, and the update/upgrade cycle.

use super::refs::{Ref, binary_name_of};
use super::release::{
    checksum_sidecar, download_bytes, extract_binary, latest_release, pick_asset, platform_target,
    release_by_tag, sha256_hex, verify_checksum,
};
use super::store::now_iso;
use super::{Manager, ManagerError, Receipt, Result};
use std::path::Path;

impl Manager {
    /// Install (or upgrade to a specific tag). The krew atomicity
    /// sequence: staging → verify → extract → receipt LAST → swap.
    pub fn install(&self, r: &Ref, force: bool) -> Result<Receipt> {
        if let Some(url) = &r.tarball {
            return self.install_tarball(r, url, force);
        }
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
        let timer = crate::provider::ui::Timer::start();
        let ui = crate::provider::ui::Ui::new();
        ui.step("Resolving", &r.repo);
        let release = match &r.tag {
            Some(tag) => release_by_tag(&r.repo, tag)?,
            None => latest_release(&r.repo)?,
        };
        ui.done(
            "Resolved",
            &format!("{repo} @ {tag}", repo = r.repo, tag = release.tag_name),
        );
        let target = platform_target();
        let asset = pick_asset(&release, target)?;
        let sidecar = checksum_sidecar(&release, asset)?;

        let spinner = ui.spinner(&format!("Downloading {}", asset.name));
        let tarball = download_bytes(&asset.browser_download_url)?;
        drop(spinner);
        ui.step("Verifying", "sha256 checksum");
        verify_checksum(&tarball, &sidecar.browser_download_url)?;
        ui.done("Verified", &format!("sha256 ok ({})", asset.name));

        ui.step("Extracting", &binary_name_of(r));
        let binary_name = format!("rootle-{}", r.name);
        let bytes = extract_binary(&tarball, &binary_name)?;
        ui.done("Extracted", &binary_name);

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

        ui.summary("Installed", &r.name, &release.tag_name, timer.elapsed());
        ui.note(&format!(
            "you are trusting {repo} — run `rootle provider use {name}` to activate",
            repo = r.repo,
            name = r.name
        ));
        Ok(receipt)
    }

    /// Plain-HTTP install (plans/0014 #1a): the URL names the platform
    /// tarball; the mandatory `.sha256` sidecar rides at
    /// `<url>.sha256`. Same krew atomicity as a release install, but
    /// no releases API — install-and-pin: `update`/`upgrade` never
    /// track these receipts (#1b).
    fn install_tarball(&self, r: &Ref, url: &str, force: bool) -> Result<Receipt> {
        let file = url.rsplit('/').next().unwrap_or(url);
        if let Some(existing) = self.receipt(&r.name)
            && existing.source == url
            && !force
        {
            return Err(ManagerError::User(format!(
                "{} is already installed from this URL (use --force to reinstall)",
                r.name
            )));
        }
        let timer = crate::provider::ui::Timer::start();
        let ui = crate::provider::ui::Ui::new();

        let spinner = ui.spinner(&format!("Downloading {file}"));
        let tarball = download_bytes(url)?;
        drop(spinner);
        ui.step("Verifying", "sha256 checksum");
        verify_checksum(&tarball, &format!("{url}.sha256"))?;
        ui.done("Verified", &format!("sha256 ok ({file})"));

        ui.step("Extracting", &binary_name_of(r));
        let binary_name = format!("rootle-{}", r.name);
        let bytes = extract_binary(&tarball, &binary_name)?;
        ui.done("Extracted", &binary_name);

        // No release tag exists on a plain-HTTP host: the filename
        // version when it has one, else the content id itself.
        let tag = r
            .tag
            .clone()
            .unwrap_or_else(|| sha256_hex(&tarball)[..12].to_string());
        let vdir = self.version_dir(&r.name, &tag);
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
            source: url.to_string(),
            tag: tag.clone(),
            sha256: sha256_hex(&tarball),
            pinned: true, // plain-HTTP installs are install-and-pin
            installed_at: now_iso(),
            latest_tag: None,
        };
        self.write_receipt(&receipt)?;
        self.point_current(&r.name, &tag)?;

        ui.summary("Installed", &r.name, &tag, timer.elapsed());
        ui.note(&format!(
            "you are trusting {url} — install-and-pin: `update`/`upgrade` do not track \
             plain-HTTP sources; upgrades come from whatever deployed it"
        ));
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
            if receipt.pinned || !tracks_releases(&receipt.source) {
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
            if !tracks_releases(&receipt.source) {
                // plans/0014 #1b: plain-HTTP and --path installs are
                // install-and-pin — upgrades come from whatever
                // deployed them, never from us.
                println!(
                    "{}: {} source is install-and-pin, not tracked by upgrade",
                    receipt.name, receipt.source
                );
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
                tarball: None,
            };
            // Force through the already-installed check.
            self.install(&r, true)?;
        }
        Ok(())
    }
}

/// Only releases-API sources (`owner/repo` on github.com) are tracked
/// by `update`/`upgrade` — plain-HTTP URLs and `--path` installs are
/// install-and-pin (plans/0014 #1b).
fn tracks_releases(source: &str) -> bool {
    !source.contains("://") && source.matches('/').count() == 1
}

#[cfg(test)]
mod tests {
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
        let r = Ref::parse(&url).unwrap();
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
        let bad = Ref::parse(&bad_url).unwrap();
        let err = manager.install(&bad, false).unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"), "got: {err}");
        assert!(manager.receipt("tampered-gitlab").is_none());

        // #1b: update/upgrade never touch plain-HTTP receipts — no
        // network call against a bogus releases URL, no state change.
        assert!(manager.update(None).unwrap().is_empty());
        manager.upgrade(None, false, true).unwrap();
        assert_eq!(manager.receipt("gitlab").unwrap().tag, "v0.1.0");
    }
}
