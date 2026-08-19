//! GitHub REST backend (PLAN.md §7). Blocking client on worker threads;
//! results return over an mpsc channel as `AppEvent`s. Auth resolution:
//! GHX_TOKEN → GITHUB_TOKEN → `gh auth token` → anonymous (60 req/h).

pub mod client;
pub mod types;

pub use client::Client;
pub use types::SearchItem;
pub use types::TreeNode;
