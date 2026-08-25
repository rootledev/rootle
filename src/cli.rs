//! CLI arguments (PLAN.md milestone 8) + the provider manager
//! subcommand tree (plans/0010 M3).

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Browse GitHub repos in your terminal — a yazi-like, modal TUI.
#[derive(Debug, Parser)]
#[command(name = "rootle", version, about)]
pub struct Cli {
    /// Open this repo directly (owner/repo), skipping the search popup.
    pub repo: Option<String>,

    /// Use this config file instead of ~/.config/rootle/config.toml.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Theme name, resolved against ~/.config/rootle/themes/<name>.toml
    /// (missing keys fall back to the embedded catppuccin-mocha).
    #[arg(long, value_name = "NAME")]
    pub theme: Option<String>,

    /// Provider management (plans/0010): install, list, update,
    /// upgrade, pin, remove, use.
    #[command(subcommand)]
    pub provider: Option<ProviderCommand>,
}

/// Install a provider from a GitHub release, manage it locally, and
/// switch the active backend.
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// Install a provider binary from GitHub releases.
    ///
    /// REF: `gitlab` (bare name, resolves to rootledev/rootle-gitlab),
    /// `owner/repo`, `https://github.com/owner/repo`, each optionally
    /// `@tag`-pinned. Checksum verification is mandatory.
    Install {
        ref_: String,

        /// Pin to the given tag (or the latest when already @tag'd).
        #[arg(long)]
        pin: bool,

        /// Reinstall even when the version is already present.
        #[arg(long)]
        force: bool,

        /// Local binary path — symlink, no network (development).
        #[arg(long, value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// List installed providers.
    List {
        /// JSON for scripting.
        #[arg(long)]
        json: bool,
    },

    /// Refresh latest-known release tags (non-mutating).
    Update {
        /// A specific provider (default: all).
        name: Option<String>,
    },

    /// Upgrade provider binaries to the latest release.
    Upgrade {
        /// A specific provider, or --all.
        name: Option<String>,

        #[arg(long)]
        all: bool,

        /// Print old → new without swapping.
        #[arg(long)]
        dry_run: bool,

        /// Upgrade pinned providers too.
        #[arg(long)]
        force: bool,
    },

    /// Freeze a provider at its current (or given) tag.
    Pin { name: String, tag: Option<String> },

    /// Unfreeze a provider (resume auto-upgrades).
    Unpin { name: String },

    /// Remove a provider binary and its receipt.
    Remove { name: String },

    /// Activate a provider: writes [provider] in config.toml.
    ///
    /// Extra argv after -- reaches the provider binary (e.g. `use
    /// gitlab -- --instance https://gitlab.example.com`).
    Use {
        name: String,
        #[arg(last = true)]
        extra: Vec<String>,
    },
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
            provider: None,
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

    #[test]
    fn parses_provider_subcommands() {
        use clap::Parser;
        let c = Cli::try_parse_from(["rootle", "provider", "list"]).unwrap();
        assert!(matches!(
            c.provider,
            Some(ProviderCommand::List { json: false })
        ));
        let c = Cli::try_parse_from(["rootle", "provider", "install", "gitlab", "--pin"]).unwrap();
        assert!(matches!(
            c.provider,
            Some(ProviderCommand::Install { ref_, pin: true, .. }) if ref_ == "gitlab"
        ));
        let c = Cli::try_parse_from([
            "rootle",
            "provider",
            "use",
            "gitlab",
            "--",
            "--instance",
            "https://gitlab.example.com",
        ])
        .unwrap();
        assert!(matches!(
            c.provider,
            Some(ProviderCommand::Use { name, extra, .. }) if name == "gitlab" && extra.len() == 2
        ));
    }
}
