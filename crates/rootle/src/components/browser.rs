//! Three-pane miller browser over org → repo → dir → file (PLAN.md §1).
//! Yazi semantics: `h` moves focus left into the parent column, where
//! j/k then browse the parent — the child column rebuilds (cascades)
//! from the new selection. `l` drills back in / deeper.
//! Org repos and repo trees arrive asynchronously from the GitHub API;
//! no mock content is ever shown for trees (honest empty until loaded).

mod loading;
mod marks;
mod navigation;
mod presentation;
mod revision;

mod blobs;
pub(crate) use blobs::CachedBlob;
pub(crate) use history::{History, HistoryDiagnostics};
pub(crate) use lenses::BlameState;

mod history;
pub(crate) mod lenses;

use super::Component;
use super::pane::{Entry, EntryKind, Pane};
use super::preview::Preview;
use super::vim_input::VimInput;
use crate::action::Action;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use std::collections::{HashMap, HashSet};

mod tree;

pub use tree::RepoTree;

pub struct Browser {
    /// Level stack: levels[i] lists the children of the selection in
    /// levels[i-1]. levels[0] = orgs, levels[1] = repos, 2 = repo root,
    /// 3+ = dirs.
    levels: Vec<Pane>,
    /// Which level owns the keyboard. Default = deepest level.
    focus: usize,
    tree: Option<RepoTree>,
    tree_source: Option<crate::request::TreeRequest>,
    tree_load: crate::request::LoadState<crate::request::TreeRequest>,
    tree_generation: crate::request::TreeGeneration,
    owner_load: crate::request::LoadState<crate::request::OwnerListRequest>,
    owner_generation: crate::request::OwnerGeneration,
    owner_entry_count: Option<usize>,
    /// A later tree open revokes the pending owner's right to replace columns.
    owner_load_owns_view: bool,
    /// Blob content by sha (in-memory, session-scoped): sanitized raw
    /// text + highlighted lines. Lines are cached so navigation never
    /// re-highlights; the raw text lets a theme switch restyle
    /// everything without a refetch.
    blobs: HashMap<String, CachedBlob>,
    /// Shas with an in-flight fetch (dedupe worker spawns).
    pending_blobs: HashSet<String>,
    /// Shas whose fetch failed, with the error — re-selecting re-shows
    /// the error instead of a "loading…" placeholder nothing resolves
    /// (0023 breaker). Cleared by explicit reload (the retry path).
    failed_blobs: HashMap<String, String>,
    /// Set by refresh when the selected file needs a fetch; the app
    /// drains it via `take_blob_request` and routes `LoadBlob`.
    blob_request: Option<(String, String)>,
    pub preview: Preview,
    /// `/` filter input, owned here, active in SEARCH mode.
    pub filter_input: VimInput,
    /// `␣ /` find-in-file input, active in FIND mode (plans/0007 §3).
    pub find_input: VimInput,
    /// VISUAL mode (plans/0004 §1): marked entries, keyed by
    /// [`MarkKey`] so marks survive cascades.
    visual: bool,
    marks: std::collections::HashSet<MarkKey>,
    /// plans/0016 M1a: the browsed revision (None = default branch).
    current_ref: Option<String>,
    /// plans/0016 M1b: the file-history lens over the preview pane.
    /// Some(_) while active; the preview survives underneath.
    history: Option<History>,
    history_generation: crate::request::HistoryGeneration,
    /// plans/0016 M1c: blame ranges for the previewed file.
    blame: Option<BlameState>,
    /// plans/0028: the commit viewer over the preview pane, entered
    /// from the history lens (`d`).
    commit: Option<crate::components::commit::CommitView>,
    /// Viewing a file at a commit (history Enter): the restore point
    /// is the present-day blob — (path, sha) — re-rendered from the
    /// in-memory cache on the way back.
    at_commit: Option<(String, String)>,
}

impl Default for Browser {
    fn default() -> Self {
        Self::new(&[])
    }
}

impl Browser {
    pub fn new(recent_orgs: &[String]) -> Self {
        let orgs = recent_orgs
            .iter()
            .map(|n| Entry::new(n, EntryKind::Owner))
            .collect();
        // Starts folded at the orgs level; the repos level arrives
        // asynchronously via `org_repos_loaded`.
        let mut browser = Browser {
            levels: vec![Pane::new("owners", orgs)],
            focus: 0,
            tree: None,
            tree_source: None,
            tree_load: Default::default(),
            tree_generation: Default::default(),
            owner_load: Default::default(),
            owner_generation: Default::default(),
            owner_entry_count: None,
            owner_load_owns_view: false,
            blobs: HashMap::new(),
            pending_blobs: HashSet::new(),
            failed_blobs: HashMap::new(),
            blob_request: None,
            preview: Preview::new(),
            filter_input: VimInput::transient(),
            find_input: VimInput::transient(),
            visual: false,
            marks: std::collections::HashSet::new(),
            current_ref: None,
            history: None,
            history_generation: crate::request::HistoryGeneration::default(),
            commit: None,
            blame: None,
            at_commit: None,
        };
        browser.sync();
        browser
    }

    /// Only a provider's explicit organization selection enables organization scope.
    pub fn selected_org(&self) -> Option<String> {
        self.levels[0]
            .selected_entry()
            .filter(|entry| entry.kind == EntryKind::Org)
            .map(|entry| entry.name.clone())
    }

    pub fn selected_owner(&self) -> Option<&str> {
        self.levels[0]
            .selected_entry()
            .map(|entry| entry.name.as_str())
    }

    /// Default branch of the open repo, if a tree is loaded.
    pub fn branch(&self) -> Option<&str> {
        self.tree.as_ref().map(|t| t.branch.as_str())
    }

    /// Repo-level entries as full names (`org/repo`), if loaded.
    pub fn org_repo_full_names(&self) -> Vec<String> {
        let Some(org) = self.selected_owner() else {
            return vec![];
        };
        let Some(repos) = self.levels.get(1) else {
            return vec![];
        };
        repos
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Repo)
            .map(|e| format!("{org}/{}", e.name))
            .collect()
    }

    /// A provider search result explicitly classified this owner as an organization.
    pub fn select_org(&mut self, org: &str) {
        self.select_owner(org);
        if let Some(entry) = self.levels[0]
            .entries
            .iter_mut()
            .find(|entry| entry.name == org)
        {
            entry.kind = EntryKind::Org;
        }
    }

    /// A repository owner or saved legacy name carries no account-type evidence.
    pub fn select_owner(&mut self, org: &str) {
        let pos = self.levels[0]
            .entries
            .iter()
            .position(|e| e.name == org)
            .unwrap_or_else(|| {
                self.levels[0]
                    .entries
                    .insert(0, Entry::new(org, EntryKind::Owner));
                0
            });
        self.levels[0].select(pos);
        self.focus = 0;
        self.sync();
    }

    /// Shared condition for applying arrived blame ranges to the open lens.
    pub(crate) fn blame_awaiting(&self, path: &str) -> bool {
        self.blame
            .as_ref()
            .is_some_and(|b| b.loading && b.path == path)
    }

    /// Column/marks/blob-cache summary for session traces (plans/0030):
    /// indices and counts only, no entry text.
    pub(crate) fn diagnostics(&self) -> BrowserDiagnostics {
        BrowserDiagnostics {
            focus: self.focus,
            columns: self.levels.iter().map(|p| p.diagnostics()).collect(),
            visual: self.visual,
            marks: self.marks.len(),
            cached_blobs: self.blobs.len(),
            pending_blobs: self.pending_blobs.len(),
            failed_blobs: self.failed_blobs.len(),
            history: self.history.as_ref().map(|h| h.diagnostics()),
            blame: self.blame.as_ref().map(|b| BlameDiagnostics {
                loading: b.loading,
                ranges: b.ranges.len(),
            }),
        }
    }
}

/// Diagnostic summary of the browser (plans/0030 session traces).
#[derive(Debug, Clone)]
pub(crate) struct BrowserDiagnostics {
    pub(crate) focus: usize,
    pub(crate) columns: Vec<super::pane::PaneDiagnostics>,
    pub(crate) visual: bool,
    pub(crate) marks: usize,
    pub(crate) cached_blobs: usize,
    pub(crate) pending_blobs: usize,
    pub(crate) failed_blobs: usize,
    pub(crate) history: Option<HistoryDiagnostics>,
    pub(crate) blame: Option<BlameDiagnostics>,
}

/// Blame-lens summary: is a fetch in flight, how many ranges landed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlameDiagnostics {
    pub(crate) loading: bool,
    pub(crate) ranges: usize,
}

pub use marks::MarkKey;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::Highlighter;
    use crate::theme::Theme;

    #[test]
    fn restyle_blobs_recolors_cached_lines_without_refetch() {
        let mut b = Browser::new(&[]);
        let mocha = Highlighter::default();
        let lines = mocha.highlight("lib.rs", "fn main() {}\n");
        b.blob_loaded("sha1", "lib.rs", "rust", "fn main() {}\n".into(), lines);
        // Sanity: cached under mocha (mauve keyword).
        assert_eq!(
            b.blobs["sha1"].lines[0].spans[0].style.fg,
            Some(ratatui::style::Color::Rgb(203, 166, 247))
        );
        let dracula = Highlighter::new(&Theme::embedded("dracula").unwrap());
        b.restyle_blobs(&dracula);
        assert_eq!(
            b.blobs["sha1"].lines[0].spans[0].style.fg,
            Some(ratatui::style::Color::Rgb(255, 121, 198)),
            "cached lines should follow the new palette from raw text"
        );
    }
}
