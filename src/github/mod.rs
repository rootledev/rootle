//! GitHub REST backend (PLAN.md §7) — wire models + raw client.
//! The UI-facing seam is `crate::provider`; `provider/github.rs` wraps this. Blocking client on worker threads;
//! results return over an mpsc channel as `AppEvent`s. Auth resolution:
//! ROOTLE_TOKEN → GITHUB_TOKEN → `gh auth token` → anonymous (60 req/h).

pub(crate) mod cache;
pub mod client;
pub mod types;

pub use client::Client;
