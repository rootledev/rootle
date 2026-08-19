//! App events from background workers (GitHub API calls run on worker
//! threads; results return over this channel into the event loop).

use crate::github::SearchItem;

#[derive(Debug)]
pub enum AppEvent {
    SearchResults {
        gen: u64,
        items: Vec<SearchItem>,
    },
    SearchFailed {
        gen: u64,
        message: String,
    },
    OrgReposLoaded {
        org: String,
        repos: Vec<String>,
    },
    OrgReposFailed {
        org: String,
        message: String,
    },
    TreeLoaded {
        owner: String,
        name: String,
        entries: Vec<crate::github::types::TreeNode>,
        truncated: bool,
    },
    TreeFailed {
        owner: String,
        name: String,
        message: String,
    },
    BlobLoaded {
        sha: String,
        name: String,
        bytes: Vec<u8>,
    },
    BlobFailed {
        sha: String,
        message: String,
    },
}

pub type AppTx = std::sync::mpsc::Sender<AppEvent>;
pub type AppRx = std::sync::mpsc::Receiver<AppEvent>;

pub fn channel() -> (AppTx, AppRx) {
    std::sync::mpsc::channel()
}
