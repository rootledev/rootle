//! ~/.config/ghx/config.toml — [editor] and [theme]. Settings view: later.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub editor: EditorConfig,
    pub theme: ThemeConfig,
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    pub program: Option<String>,
    pub args: Vec<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    /// Blob cache size cap in MiB; oldest (least-recently-used) blobs
    /// are evicted past it.
    pub max_mb: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub path: Option<PathBuf>,
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
        dirs::config_dir().map(|d| d.join("ghx").join("config.toml"))
    }

    /// Load config; missing or malformed → defaults (never fail startup).
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }
}
