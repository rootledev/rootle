//! Provider composition root (plans/0005, 0024 M5): the one place
//! concrete implementations meet the app. `build()` reads the config
//! and hands back an `Arc<dyn Provider>` — the trait, wire types, and
//! errors live in the `rootle-provider` crate; the implementations in
//! `rootle-github` (in-tree reference) and `rootle-stdio` (external
//! NDJSON-RPC children); the binary manager in `rootle-manager`.
//! The seam's vocabulary is re-exported here so app code keeps one
//! import site (`crate::provider::*`).

pub mod bookkeeping;

pub use rootle_provider::*;

use crate::config::Config;
use std::sync::Arc;

/// The provider's cache-subtree name from its argv: the binary's full
/// file stem (rootle-gitlab → rootle-gitlab) — matching the protocol
/// doc's `providers/<name>/` convention adapters document as their
/// default, so the handshake's cache_dir and the adapter's own
/// default are the same directory.
fn name_from_command(command: &[String]) -> String {
    command
        .first()
        .and_then(|c| std::path::Path::new(c).file_stem().and_then(|s| s.to_str()))
        .unwrap_or("provider")
        .to_string()
}

/// A config-declared provider that isn't installed on this machine
/// (plans/0019 M2) — surfaced to the app for the consent flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// The receipt/install name (`gitlab`, `rootle-gitlab`, …).
    pub name: String,
    /// "owner/repo" — a releases-API source only; a config naming a
    /// plain-HTTP tarball never reaches a Declaration.
    pub repo: String,
    /// Pin: install exactly this tag.
    pub tag: Option<String>,
    /// Integrity pin: verify the tarball against this sha256 too.
    pub sha: Option<String>,
}

/// How `build()` landed (plans/0019 M2, 0022).
#[derive(Debug)]
pub enum BuildOutcome {
    /// The configured provider is up.
    Ready,
    /// Up with a warning (github fallback when the configured one
    /// failed — never silent, never blocking; 0022 M1 makes the
    /// notice sticky).
    Warn(String),
    /// 0022 M2: the configured provider exists but won't start — the
    /// health prompt (retry / browse github / edit config).
    Health(HealthIssue),
    /// A declared provider is missing; github is carrying the session
    /// pending the consent popup.
    Missing(Declaration),
}

/// A provider that exists on disk (or in config) but fails to start —
/// the health prompt's payload (0022 M2).
#[derive(Debug, Clone)]
pub struct HealthIssue {
    /// What the config names (display name).
    pub name: String,
    /// Why it failed (spawn/parse error text).
    pub error: String,
    /// Does retry make sense? false for malformed/tarball kinds —
    /// retrying a typo fixes nothing.
    pub retryable: bool,
}

/// Build the configured provider. Invalid/unsupported config falls
/// back to GitHub (with a warning for the status line) — a provider
/// misconfiguration must never block startup.
pub fn build(config: &Config) -> (Arc<dyn Provider>, BuildOutcome) {
    match config.provider.kind.as_str() {
        "github" => (
            Arc::new(rootle_github::GitHubProvider::new(config.cache.max_mb)),
            BuildOutcome::Ready,
        ),
        "stdio" => {
            let (provider, warn) = try_spawn_stdio(config, config.provider.command.clone(), None);
            match provider {
                Ok(p) => (p, warn.map_or(BuildOutcome::Ready, BuildOutcome::Warn)),
                Err(e) => (
                    github_fallback(config),
                    BuildOutcome::Health(HealthIssue {
                        name: "stdio".into(),
                        error: e,
                        retryable: true,
                    }),
                ),
            }
        }
        other => build_declared(config, other),
    }
}

/// The declared kind (plans/0019 M2): a receipt name, a bare
/// first-party name (`gitlab` → `rootledev/rootle-gitlab` via the
/// Ref grammar's rootle- convention), or an `owner/repo` slug.
/// Installed → spawn the `current` binary; missing → the consent
/// flow. Plain-HTTP tarball refs are never auto-fetched.
fn build_declared(config: &Config, kind: &str) -> (Arc<dyn Provider>, BuildOutcome) {
    let r = match rootle_manager::Ref::parse(kind) {
        Ok(r) => r,
        Err(e) => {
            return (
                github_fallback(config),
                BuildOutcome::Health(HealthIssue {
                    name: kind.to_string(),
                    error: e.to_string(),
                    retryable: false,
                }),
            );
        }
    };
    if r.tarball.is_some() {
        return (
            github_fallback(config),
            BuildOutcome::Health(HealthIssue {
                name: kind.to_string(),
                error: "kind names a plain-HTTP tarball — install-and-pin only (run `rootle provider install` with the URL)".into(),
                retryable: false,
            }),
        );
    }
    let Ok(m) = rootle_manager::Manager::new() else {
        return (
            github_fallback(config),
            BuildOutcome::Warn("no provider data dir; using github".into()),
        );
    };
    match m.current_binary(&r.name) {
        Some(bin) => {
            // Extra argv (beyond the binary) from a hand-written
            // config still rides along.
            let mut argv = vec![bin.display().to_string()];
            argv.extend(config.provider.command.iter().cloned());
            match try_spawn_stdio(config, argv, Some(r.name.clone())).0 {
                Ok(p) => (p, BuildOutcome::Ready),
                Err(e) => (
                    github_fallback(config),
                    BuildOutcome::Health(HealthIssue {
                        name: kind.to_string(),
                        error: e,
                        retryable: true,
                    }),
                ),
            }
        }
        None => (
            github_fallback(config),
            BuildOutcome::Missing(Declaration {
                name: r.name,
                repo: r.repo,
                tag: config.provider.tag.clone().or(r.tag),
                sha: config.provider.sha.clone(),
            }),
        ),
    }
}

/// Spawn an installed declared provider by name — the 0019 M2
/// hot-swap after the consent install lands.
pub fn spawn_installed(config: &Config, name: &str) -> Result<Arc<dyn Provider>, String> {
    let bin = rootle_manager::Manager::new()
        .ok()
        .and_then(|m| m.current_binary(name))
        .ok_or_else(|| format!("{name} installed but its current binary is missing"))?;
    let mut argv = vec![bin.display().to_string()];
    argv.extend(config.provider.command.iter().cloned());
    try_spawn_stdio(config, argv, Some(name.to_string())).0
}

fn github_fallback(config: &Config) -> Arc<dyn Provider> {
    Arc::new(rootle_github::GitHubProvider::new(config.cache.max_mb))
}

/// The stdio spawn shared by the `stdio` kind and declared providers.
/// Returns (result, warning): the warning covers an unrecognized
/// `stderr` value (a typo shouldn't silently disable debugging).
fn try_spawn_stdio(
    config: &Config,
    argv: Vec<String>,
    cache_name: Option<String>,
) -> (Result<Arc<dyn Provider>, String>, Option<String>) {
    let stderr = config.provider.stderr.trim();
    let inherit = stderr == "inherit";
    let warn = (!inherit && !stderr.is_empty() && stderr != "null").then(|| {
        format!(
            "provider stderr {stderr:?} not recognized (use \"inherit\" or \"null\"); discarding child stderr"
        )
    });
    // The user's cache budget and this provider's subtree travel in
    // every initialize (protocol v1.2, advisory) — one [cache] max_mb
    // knob governs every backend.
    let cache_bytes = config.cache.max_mb * 1024 * 1024;
    let cache_dir = crate::paths::cache_dir().map(|d| {
        d.join("rootle")
            .join("providers")
            .join(cache_name.unwrap_or_else(|| name_from_command(&argv)))
    });
    match rootle_stdio::StdioProvider::spawn_with_cache(
        &argv,
        std::time::Duration::from_millis(config.provider.timeout_ms),
        inherit,
        cache_bytes,
        cache_dir,
    ) {
        Ok(p) => (Ok(Arc::new(p)), warn),
        Err(e) => (Err(e.to_string()), warn),
    }
}

/// Offline provider for tests: every call errors, nothing spawns.
pub fn offline() -> Arc<dyn Provider> {
    struct Offline;
    impl Provider for Offline {
        fn name(&self) -> &str {
            "offline"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                orgs: false,
                code_search: false,
                file_search: false,
                // Tests inject the v1.5/v1.6 events directly (the
                // calls themselves error offline) — declare the caps
                // so the lenses open.
                refs: true,
                log: true,
                blame: true,
                commit: true,
            }
        }
        fn search(&self, _: &str) -> ProviderResult<Vec<SearchItem>> {
            Err("offline".into())
        }
        fn org_repos(&self, _: &str) -> ProviderResult<Vec<RepoInfo>> {
            Err("offline".into())
        }
        fn fetch_tree(&self, _: &RepoId, _: Option<&GitRef>) -> ProviderResult<TreeResult> {
            Err("offline".into())
        }
        fn fetch_blob(&self, _: &RepoId, _: &Sha) -> ProviderResult<Vec<u8>> {
            Err("offline".into())
        }
        fn search_code(&self, _: &str) -> ProviderResult<SearchCodeResult> {
            Err("offline".into())
        }
        fn clone_url(&self, _: &RepoId) -> ProviderResult<String> {
            Err("offline".into())
        }
        fn web_url(
            &self,
            _: &RepoId,
            _: &str,
            _: Option<&GitRef>,
            _: Option<u32>,
            _: Option<u32>,
            _: bool,
        ) -> ProviderResult<String> {
            Err("offline".into())
        }
        fn org_url(&self, _: &str) -> ProviderResult<String> {
            Err("offline".into())
        }
    }
    Arc::new(Offline)
}
