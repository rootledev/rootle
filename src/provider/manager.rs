//! Provider manager (plans/0010): install, list, update, upgrade,
//! pin, remove, use — for stdio provider binaries distributed as
//! GitHub release tarballs (the 4-target matrix rootle and its
//! providers ship: linux+macOS × x86_64+aarch64).
//!
//! Layout (XDG tiers, one purpose each — see the plan's survey):
//!
//! ```text
//! ~/.local/share/rootle/providers/<name>/<version>/rootle-<name>  binary
//! ~/.local/share/rootle/providers/<name>/current -> <version>/    pointer
//! ~/.local/state/rootle/providers/<name>.toml                    receipt
//! ```
//!
//! Atomicity (krew model): download to temp staging → verify sha256
//! (mandatory) → extract into the versioned dir → write the receipt
//! LAST → atomically re-point `current`. A failed step leaves the
//! previous version intact. The running TUI is never affected —
//! provider children die with the app, the next spawn picks up the
//! new `current`.
//!
//! Config declares, the store installs (mise pattern): `use` writes
//! the existing `[provider]` block; the runtime `build()` is
//! untouched and hand-edited configs keep working.
//!
//! Submodules by concern: `refs` (the install-reference grammar),
//! `release` (GitHub API, tarball, checksum), `install` (the
//! install/update/upgrade flows), `store` (local state on disk),
//! `bookkeeping` (pin/remove/use/list).

mod bookkeeping;
mod install;
mod refs;
mod release;
mod store;

pub use refs::Ref;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The receipt: provenance recorded beside the binary so list/upgrade
/// read local state, never the network (gh manifest.yml pattern).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub name: String,
    /// "owner/repo"
    pub source: String,
    pub tag: String,
    pub sha256: String,
    pub pinned: bool,
    #[serde(default)]
    pub installed_at: Option<String>,
    /// Latest tag known from `update` (krew's non-mutating refresh).
    #[serde(default)]
    pub latest_tag: Option<String>,
}

type Result<T, E = ManagerError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("{0}")]
    User(String),
    #[error("network: {0}")]
    Network(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Manager {
    store: PathBuf,
    state: PathBuf,
}

impl Manager {
    pub fn new() -> Result<Manager> {
        Ok(Manager {
            store: store::store_root().ok_or_else(|| ManagerError::User("no data dir".into()))?,
            state: store::state_root().ok_or_else(|| ManagerError::User("no state dir".into()))?,
        })
    }
}

/// An installed provider, as `list` reports.
#[derive(Debug, Clone)]
pub struct Installed {
    pub receipt: Receipt,
    pub active: bool,
    pub current: Option<String>,
}
