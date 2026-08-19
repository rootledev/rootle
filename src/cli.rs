//! CLI arguments (PLAN.md milestone 8).

use clap::Parser;
use std::path::PathBuf;

/// Browse GitHub repos in your terminal — a yazi-like, modal TUI.
#[derive(Debug, Parser)]
#[command(name = "ghx", version, about)]
pub struct Cli {
    /// Open this repo directly (owner/repo), skipping the search popup.
    pub repo: Option<String>,

    /// Use this config file instead of ~/.config/ghx/config.toml.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Theme name, resolved against ~/.config/ghx/themes/<name>.toml
    /// (missing keys fall back to the embedded catppuccin-mocha).
    #[arg(long, value_name = "NAME")]
    pub theme: Option<String>,
}

impl Cli {
    /// Split the positional repo argument into (owner, name), if valid.
    pub fn repo_parts(&self) -> Option<(String, String)> {
        let repo = self.repo.as_ref()?;
        let (owner, name) = repo.split_once('/')?;
        if owner.is_empty() || name.is_empty() {
            return None;
        }
        Some((owner.to_string(), name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(repo: Option<&str>) -> Cli {
        Cli {
            repo: repo.map(str::to_string),
            config: None,
            theme: None,
        }
    }

    #[test]
    fn parses_owner_repo() {
        assert_eq!(
            cli(Some("ratatui/ratatui")).repo_parts(),
            Some(("ratatui".into(), "ratatui".into()))
        );
    }

    #[test]
    fn rejects_malformed_repo() {
        assert_eq!(cli(Some("noslash")).repo_parts(), None);
        assert_eq!(cli(Some("/empty")).repo_parts(), None);
        assert_eq!(cli(None).repo_parts(), None);
    }
}
