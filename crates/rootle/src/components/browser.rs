//! Three-pane miller browser over org → repo → dir → file (PLAN.md §1).
//! Yazi semantics: `h` moves focus left into the parent column, where
//! j/k then browse the parent — the child column rebuilds (cascades)
//! from the new selection. `l` drills back in / deeper.
//! Org repos and repo trees arrive asynchronously from the GitHub API;
//! no mock content is ever shown for trees (honest empty until loaded).

mod marks;
mod navigation;
mod presentation;
mod revision;

mod blobs;
pub(crate) use blobs::CachedBlob;
pub(crate) use lenses::{BlameState, History};

pub(crate) mod lenses;

use super::Component;
use super::pane::{Entry, EntryKind, Pane};
use super::preview::Preview;
use super::vim_input::VimInput;
use crate::action::Action;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use rootle_provider::TreeNode;
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
        Self::new(&[], &[])
    }
}

impl Browser {
    pub fn new(recent_orgs: &[String], defaults: &[String]) -> Self {
        let mut names: Vec<String> = recent_orgs.to_vec();
        for d in defaults {
            if !names.iter().any(|n| n == d) {
                names.push(d.to_string());
            }
        }
        let orgs = names
            .iter()
            .map(|n| Entry::new(n, EntryKind::Org))
            .collect();
        // Starts folded at the orgs level; the repos level arrives
        // asynchronously via `org_repos_loaded`.
        let mut browser = Browser {
            levels: vec![Pane::new("orgs", orgs)],
            focus: 0,
            tree: None,
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
            commit: None,
            blame: None,
            at_commit: None,
        };
        browser.sync();
        browser
    }

    /// Selected org at the top level, if any.
    pub fn selected_org(&self) -> Option<String> {
        self.levels[0].selected_entry().map(|e| e.name.clone())
    }

    /// Default branch of the open repo, if a tree is loaded.
    pub fn branch(&self) -> Option<&str> {
        self.tree.as_ref().map(|t| t.branch.as_str())
    }

    /// Repo-level entries as full names (`org/repo`), if loaded.
    pub fn org_repo_full_names(&self) -> Vec<String> {
        let Some(org) = self.selected_org() else {
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

    /// Ensure `org` exists in the orgs level and select it.
    pub fn select_org(&mut self, org: &str) {
        let pos = self.levels[0]
            .entries
            .iter()
            .position(|e| e.name == org)
            .unwrap_or_else(|| {
                self.levels[0]
                    .entries
                    .insert(0, Entry::new(org, EntryKind::Org));
                0
            });
        self.levels[0].select(pos);
        self.focus = 0;
        self.sync();
    }

    /// Org repos arrived from the API: install/replace the repos level.
    /// Ignored if the user has since selected a different org.
    pub fn org_repos_loaded(&mut self, org: &str, repos: Vec<rootle_provider::RepoInfo>) {
        if self.selected_org().as_deref() != Some(org) {
            return;
        }
        let entries = repos
            .iter()
            .map(|r| Entry::new(&r.name, EntryKind::Repo))
            .collect();
        self.levels.truncate(1);
        self.levels.push(Pane::new(org, entries));
        self.focus = 1;
        self.cascade();
        self.sync();
    }

    /// Repo tree arrived: install it and rebuild the dir columns.
    /// Ignored if it doesn't match the currently selected repo.
    pub fn tree_loaded(
        &mut self,
        owner: &str,
        name: &str,
        entries: Vec<TreeNode>,
        truncated: bool,
        branch: String,
    ) {
        if self.levels.get(1).map(|p| p.title.as_str()) != Some(owner) {
            return;
        }
        let current_repo = self.levels[1].selected_entry().map(|e| e.name.clone());
        if current_repo.as_deref() != Some(name) {
            return;
        }
        self.tree = Some(RepoTree::new(
            owner.to_string(),
            name.to_string(),
            truncated,
            branch,
            entries,
        ));
        self.levels.truncate(2);
        self.focus = 1;
        self.cascade();
        // Complete the interrupted drill: the tree load was requested by
        // acting on this repo, so land the user in its root pane rather
        // than leaving them on the repos pane to press `l` again.
        if self.levels.len() > 2 {
            self.focus = 2;
        }
        self.sync();
    }
}

pub use marks::MarkKey;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::Highlighter;
    use crate::theme::Theme;

    #[test]
    fn restyle_blobs_recolors_cached_lines_without_refetch() {
        let mut b = Browser::new(&[], &[]);
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
