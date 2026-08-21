//! App events from background workers (GitHub API calls run on worker
//! threads; results return over this channel into the event loop).

use crate::provider::SearchItem;

#[derive(Debug)]
pub enum AppEvent {
    SearchResults {
        gen_id: u64,
        items: Vec<SearchItem>,
    },
    SearchFailed {
        gen_id: u64,
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
        entries: Vec<crate::provider::TreeNode>,
        truncated: bool,
        branch: String,
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
    /// Global search view results (plans/0002 §4): raw hits from the
    /// worker, styled on the UI thread.
    GlobalSearchResults {
        gen_id: u64,
        hits: Vec<crate::components::global_search::RawHit>,
    },
    GlobalSearchFailed {
        gen_id: u64,
        message: String,
    },
    /// Lazy per-hit context (plans/0006 §1): blob fetched + located on
    /// a worker for the selected bare hit.
    HitContextLoaded {
        gen_id: u64,
        repo: String,
        path: String,
        sha: String,
        line: u32,
        preview: Vec<(u32, String)>,
        match_count: u32,
        query: String,
    },
    CloneDone {
        ok: Vec<String>,
        failed: Vec<(String, String)>,
    },
    /// Org marks expanded to their repos; open the clone wizard with
    /// the combined list (plans/0004).
    CloneExpanded {
        repos: Vec<String>,
        errors: Vec<String>,
    },
}

pub type AppTx = std::sync::mpsc::Sender<AppEvent>;
pub type AppRx = std::sync::mpsc::Receiver<AppEvent>;

pub fn channel() -> (AppTx, AppRx) {
    std::sync::mpsc::channel()
}
