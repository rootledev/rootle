//! Lifecycle after install: pin/unpin, remove, `use` (the config
//! write), and `list`.

use super::{Installed, Manager, ManagerError, Result};
use crate::config::Config;

impl Manager {
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

    /// mise's `use`: write the active provider into config.toml —
    /// the 0019 M2 declarative form (`kind = <name>`, `tag`/`sha`
    /// when pinned, `command` = extra argv). No materialized binary
    /// path: a synced config resolves on any machine, and a missing
    /// one gets the consent flow instead of a silent fallback.
    pub fn activate(&self, name: &str, extra_argv: &[String]) -> Result<()> {
        let receipt = self
            .receipt(name)
            .ok_or_else(|| ManagerError::User(format!("{name} is not installed")))?;
        let mut config = Config::load();
        config.provider.kind = name.to_string();
        config.provider.command = extra_argv.to_vec();
        config.provider.tag = receipt.pinned.then(|| receipt.tag.clone());
        config.provider.sha = receipt.pinned.then(|| receipt.sha256.clone());
        config
            .save()
            .map_err(|e| ManagerError::User(format!("save config: {e}")))?;
        let ui = crate::provider::ui::Ui::new();
        ui.summary("Activated", name, "", std::time::Duration::from_secs(0));
        ui.note("restart rootle to apply");
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
                // 0019 M2: active is the declared kind's name, or the
                // legacy stdio argv pointing into the store.
                let active = config.provider.kind == receipt.name
                    || active_command.as_ref().is_some_and(|cmd| {
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
}
