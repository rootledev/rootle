//! Local state on disk: the XDG roots, receipts, the `current`
//! pointer, and receipt timestamps. Everything here touches the
//! filesystem — never the network.

use super::{Manager, ManagerError, Receipt, Result};
use std::path::PathBuf;
use std::time::SystemTime;

/// Where installed binaries live (XDG data).
pub(super) fn store_root() -> Option<PathBuf> {
    crate::paths::data_dir().map(|d| d.join("rootle").join("providers"))
}

/// Where receipts live (XDG state).
pub(super) fn state_root() -> Option<PathBuf> {
    crate::paths::state_dir().map(|d| d.join("rootle").join("providers"))
}

impl Manager {
    pub(super) fn receipt_path(&self, name: &str) -> PathBuf {
        self.state.join(format!("{name}.toml"))
    }

    pub(super) fn version_dir(&self, name: &str, tag: &str) -> PathBuf {
        self.store.join(name).join(tag)
    }

    pub(super) fn current_link(&self, name: &str) -> PathBuf {
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

    pub(super) fn write_receipt(&self, receipt: &Receipt) -> Result<()> {
        std::fs::create_dir_all(&self.state)?;
        let path = self.receipt_path(&receipt.name);
        let tmp = path.with_extension("toml.tmp");
        let text =
            toml::to_string_pretty(receipt).map_err(|e| ManagerError::User(e.to_string()))?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path).map_err(ManagerError::Io)?;
        Ok(())
    }

    pub(super) fn point_current(&self, name: &str, tag: &str) -> Result<()> {
        let link = self.current_link(name);
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(format!("{tag}/"), &link).map_err(ManagerError::Io)?;
        Ok(())
    }
}

pub(super) fn now_iso() -> Option<String> {
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
    fn iso_date_math() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20690), (2026, 8, 25));
    }
}
