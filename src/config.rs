//! ~/.config/rootle/config.toml — [editor], [theme], [cache],
//! [provider], [ui]. Settings view edits a subset live.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub editor: EditorConfig,
    pub theme: ThemeConfig,
    pub cache: CacheConfig,
    pub provider: ProviderConfig,
    pub ui: UiConfig,
}

/// Backend selection (plans/0005): built-in GitHub by default, or an
/// external stdio provider (NDJSON-RPC child process).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// "github" | "stdio"
    pub kind: String,
    /// argv for kind = "stdio"
    pub command: Vec<String>,
    /// Per-request read deadline in milliseconds (plans/0008 §1): a
    /// hung backend call fails instead of wedging the provider.
    pub timeout_ms: u64,
    /// Child stderr for kind = "stdio" (plans/0008 §4): "null"
    /// (default, discarded) or "inherit" (pass through — adapter
    /// debugging without a log file).
    pub stderr: String,
    /// Short display name for the modeline's forge chip
    /// (kind = "stdio"); defaults to the provider's self-reported
    /// handshake name.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct EditorConfig {
    pub program: Option<String>,
    pub args: Vec<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct CacheConfig {
    /// Blob cache size cap in MiB; oldest (least-recently-used) blobs
    /// are evicted past it.
    pub max_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub path: Option<PathBuf>,
}
/// Chrome preferences.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct UiConfig {
    /// Border corner style: "plain" (default) | "rounded" | "thick" |
    /// "double". Unknown values fall back to plain.
    pub border: String,
    /// Nerd Font glyphs (powerline arrows, forge icons) in the
    /// modeline — false keeps unicode fallbacks (❯ separators, no
    /// icons) so non-Nerd-Font terminals never see tofu.
    pub nerd_font: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Config::default().ui
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            editor: EditorConfig {
                program: None,
                args: Vec::new(),
                read_only: true,
            },
            theme: ThemeConfig {
                name: "catppuccin-mocha".into(),
                path: None,
            },
            cache: CacheConfig { max_mb: 512 },
            provider: ProviderConfig::default(),
            ui: UiConfig {
                border: "plain".into(),
                nerd_font: false,
            },
        }
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        ProviderConfig {
            kind: "github".into(),
            command: Vec::new(),
            timeout_ms: 30_000,
            stderr: "null".into(),
            name: None,
        }
    }
}

impl Default for EditorConfig {
    fn default() -> Self {
        Config::default().editor
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Config::default().theme
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Config::default().cache
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("rootle").join("config.toml"))
    }

    /// Load config; missing or malformed → defaults (never fail startup).
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        Self::load_from(&path)
    }

    /// Write the config back atomically (settings popup save).
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(()); // no config dir — nothing to persist
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path.with_extension("toml.tmp"), text)?;
        std::fs::rename(path.with_extension("toml.tmp"), path)
    }

    /// Load from an explicit path (--config); missing/malformed → defaults.
    pub fn load_from(path: &std::path::Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }
}
