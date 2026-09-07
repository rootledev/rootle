//! Lifecycle after install, the app-coupled half (plans/0024 M5):
//! `activate` and `list` read/write the app's `Config`, so they live
//! here beside the composition root instead of the `rootle-manager`
//! crate. `pin`/`unpin`/`remove` are pure receipt operations and
//! moved into the crate (`Manager` methods).

use crate::config::Config;
use rootle_manager::{Installed, Manager, ManagerError, Result};

/// mise's `use`: write the active provider into config.toml —
/// the 0019 M2 declarative form (`kind = <name>`, `tag`/`sha`
/// when pinned, `command` = extra argv). No materialized binary
/// path: a synced config resolves on any machine, and a missing
/// one gets the consent flow instead of a silent fallback.
pub fn activate(manager: &Manager, name: &str, extra_argv: &[String]) -> Result<()> {
    let receipt = manager
        .receipt(name)
        .ok_or_else(|| ManagerError::User(format!("{name} is not installed")))?;
    let (mut config, _) = Config::load();
    config.provider.kind = name.to_string();
    config.provider.command = extra_argv.to_vec();
    config.provider.tag = receipt.pinned.then(|| receipt.tag.clone());
    config.provider.sha = receipt.pinned.then(|| receipt.sha256.clone());
    config
        .save()
        .map_err(|e| ManagerError::User(format!("save config: {e}")))?;
    let ui = rootle_manager::progress::ProgressOutput::new();
    ui.summary("Activated", name, "", std::time::Duration::from_secs(0));
    ui.note("restart rootle to apply");
    Ok(())
}

/// Everything `list` shows, including the ACTIVE row.
pub fn list_installed(manager: &Manager) -> Vec<Installed> {
    let (config, _) = Config::load();
    let active_command = if config.provider.kind == "stdio" {
        config.provider.command.first().cloned()
    } else {
        None
    };
    manager
        .receipts()
        .into_iter()
        .map(|receipt| {
            let current = std::fs::read_link(manager.current_link(&receipt.name))
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
