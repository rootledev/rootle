//! XDG base dirs on **every** platform, macOS included.
//!
//! `dirs::config_dir()` & friends ignore the XDG env vars on macOS
//! (~/Library/Application Support instead of ~/.config) — but the
//! docs, install.sh, and the e2e sandbox all promise the XDG layout,
//! and the 0023 macOS CI job caught the divergence (settings
//! write-back "vanished" into ~/Library). One convention, everywhere:
//! the XDG var when set, the XDG default (~/.config, …) otherwise.

use std::path::PathBuf;

fn xdg(var: &str, default: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(default)))
}

/// $XDG_CONFIG_HOME or ~/.config
pub fn config_dir() -> Option<PathBuf> {
    xdg("XDG_CONFIG_HOME", ".config")
}

/// $XDG_CACHE_HOME or ~/.cache
pub fn cache_dir() -> Option<PathBuf> {
    xdg("XDG_CACHE_HOME", ".cache")
}

/// $XDG_STATE_HOME or ~/.local/state
pub fn state_dir() -> Option<PathBuf> {
    xdg("XDG_STATE_HOME", ".local/state")
}

/// $XDG_DATA_HOME or ~/.local/share
pub fn data_dir() -> Option<PathBuf> {
    xdg("XDG_DATA_HOME", ".local/share")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_vars_fall_back_to_xdg_defaults() {
        // Env-safe: the test binary never sets these (the sandbox
        // does it per child process, not in-process).
        for (var, default) in [
            ("XDG_CONFIG_HOME", ".config"),
            ("XDG_CACHE_HOME", ".cache"),
            ("XDG_STATE_HOME", ".local/state"),
            ("XDG_DATA_HOME", ".local/share"),
        ] {
            assert!(
                std::env::var_os(var).is_none(),
                "{var} must not leak into tests"
            );
            let home = dirs::home_dir().unwrap();
            assert_eq!(xdg(var, default), Some(home.join(default)));
        }
    }
}
