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
}
