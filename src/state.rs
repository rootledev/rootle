//! Persisted app state: ~/.local/state/ghx/state.json (PLAN.md §10).
//! Distinct from the cache — state survives cache eviction. Atomic
//! tmp+rename writes on state transitions; corrupt/missing → defaults.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

const RECENT_CAP: usize = 20;
const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub last_org: Option<String>,
    /// "owner/name"
    pub last_repo: Option<String>,
    pub last_path: Option<String>,
    pub recent_repos: Vec<String>,
    pub recent_orgs: Vec<String>,
}

impl State {
    pub fn path() -> Option<PathBuf> {
        dirs::state_dir()
            .or_else(dirs::data_dir)
            .map(|d| d.join("ghx").join("state.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let state: Self = serde_json::from_str(&text).unwrap_or_default();
        if state.version > VERSION {
            return Self::default(); // future schema — don't guess
        }
        state
    }

    pub fn save(&self) {
        if let Some(path) = Self::path() {
            let _ = self.save_to(&path); // persistence must never crash the app
        }
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut state = self.clone();
        state.version = VERSION;
        std::fs::write(&tmp, serde_json::to_string_pretty(&state)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn record_repo(&mut self, owner: &str, name: &str) {
        let full = format!("{owner}/{name}");
        self.last_org = Some(owner.to_string());
        self.last_repo = Some(full.clone());
        push_recent(&mut self.recent_repos, full);
        push_recent(&mut self.recent_orgs, owner.to_string());
    }

    pub fn record_org(&mut self, org: &str) {
        push_recent(&mut self.recent_orgs, org.to_string());
    }
}

/// LRU push: dedupe, most-recent-first, capped.
fn push_recent(list: &mut Vec<String>, item: String) {
    list.retain(|i| i != &item);
    list.insert(0, item);
    list.truncate(RECENT_CAP);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ghx-state-{}-{tag}.json", std::process::id()))
    }

    #[test]
    fn round_trip() {
        let path = temp_path("roundtrip");
        let mut state = State::default();
        state.record_repo("ratatui", "ratatui");
        state.save_to(&path).unwrap();

        let loaded = State::load_from(&path);
        assert_eq!(loaded.last_repo.as_deref(), Some("ratatui/ratatui"));
        assert_eq!(loaded.recent_orgs, vec!["ratatui"]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{ not json").unwrap();
        let loaded = State::load_from(&path);
        assert!(loaded.last_repo.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let loaded = State::load_from(&temp_path("missing"));
        assert!(loaded.last_repo.is_none());
    }

    #[test]
    fn recents_are_lru_deduped_and_capped() {
        let mut state = State::default();
        for i in 0..25 {
            state.record_repo("org", &format!("repo{i}"));
        }
        assert_eq!(state.recent_repos.len(), RECENT_CAP);
        assert_eq!(state.recent_repos[0], "org/repo24");

        state.record_repo("org", "repo10");
        assert_eq!(state.recent_repos[0], "org/repo10");
        assert_eq!(
            state
                .recent_repos
                .iter()
                .filter(|r| *r == "org/repo10")
                .count(),
            1,
            "re-selection must dedupe"
        );
    }

    #[test]
    fn future_schema_version_falls_back_to_defaults() {
        let path = temp_path("version");
        std::fs::write(&path, r#"{"version": 999, "last_repo": "a/b"}"#).unwrap();
        assert!(State::load_from(&path).last_repo.is_none());
        let _ = std::fs::remove_file(&path);
    }
}
