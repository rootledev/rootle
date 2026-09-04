//! Three-pane miller browser over org → repo → dir → file (PLAN.md §1).
//! Yazi semantics: `h` moves focus left into the parent column, where
//! j/k then browse the parent — the child column rebuilds (cascades)
//! from the new selection. `l` drills back in / deeper.
//! Org repos and repo trees arrive asynchronously from the GitHub API;
//! no mock content is ever shown for trees (honest empty until loaded).

mod blobs;
pub(crate) use blobs::CachedBlob;
pub(crate) use lenses::{BlameState, History};

pub(crate) mod lenses;

use super::pane::{Entry, EntryKind, Pane};
use super::preview::Preview;
use super::scrollbar;
use super::vim_input::VimInput;
use crate::action::Action;
use crate::provider::TreeNode;
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
    /// VISUAL mode (plans/0004 §1): marked entries, keyed
    /// `"<pane title>/<entry name>"` so marks survive cascades.
    visual: bool,
    marks: std::collections::HashSet<String>,
    /// plans/0016 M1a: the browsed revision (None = default branch).
    current_ref: Option<String>,
    /// plans/0016 M1b: the file-history lens over the preview pane.
    /// Some(_) while active; the preview survives underneath.
    history: Option<History>,
    /// plans/0016 M1c: blame ranges for the previewed file.
    blame: Option<BlameState>,
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

    /// The browsed revision (plans/0016 M1a), if switched off the
    /// default branch.
    pub fn current_ref(&self) -> Option<&str> {
        self.current_ref.as_deref()
    }

    /// Live-preview or commit a revision switch — the crumb follows;
    /// the tree refetch is the app's (LoadRepoTree reads this).
    /// Switching invalidates the revision lenses: history and blame
    /// were fetched for the previous ref.
    pub fn set_current_ref(&mut self, name: Option<String>) {
        self.current_ref = name;
        self.history = None;
        self.blame = None;
        self.preview.set_blame(None);
        self.at_commit = None;
    }

    /// Preview submode keys (plans/0016 M1, `␣ p`): the vim vertical
    /// motions are owned by `Preview::motion_key` (counts, gg/G,
    /// pages, paragraphs, %, zt/zz/zb); everything else maps through
    /// the named table in keymap.rs (hint rows derive from it).
    pub fn preview_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> Action {
        use ratatui::crossterm::event::KeyCode;
        if self.preview.motion_key(key) {
            return Action::Noop;
        }
        match key.code {
            // vim V (pane-local): anchor at the cursor, motions extend.
            KeyCode::Char('v') => {
                self.preview.toggle_visual();
                Action::Noop
            }
            // Y copies content — the selection, else the cursor line.
            KeyCode::Char('Y') => Action::PreviewCopy,
            // The ladder: Esc clears a selection before exiting.
            KeyCode::Esc if self.preview.clear_visual() => Action::Noop,
            _ => crate::keymap::preview_named(key.code),
        }
    }

    /// The yank anchor: the visual range when one is live, else the
    /// cursor line alone — web_url takes (line, end).
    pub fn yank_anchor(&self) -> (Option<u32>, Option<u32>) {
        match self.preview.visual_range() {
            Some((lo, hi)) => (Some(lo), Some(hi)),
            None => (self.preview.line(), None),
        }
    }

    pub fn close_history(&mut self) {
        self.history = None;
    }

    /// Entering open-at-commit: save the present-day blob's identity
    /// (the tree cursor is on the file the lens serves).
    pub fn note_commit_view(&mut self) {
        self.at_commit = self.selected_file();
        // Present-day blame marks are stale over a commit's content.
        self.preview.set_blame(None);
    }

    /// Show a file at a commit (v1.5): bypass refresh_preview — the
    /// tree cursor's sha is the present-day one — but cache the bytes
    /// so switching back and forth stays free.
    #[allow(clippy::too_many_arguments)]
    /// VISUAL mode on: checkboxes appear on every pane.
    pub fn enter_visual(&mut self) {
        self.visual = true;
        self.sync();
    }

    /// VISUAL off; marks are kept (a following `:clone` consumes them).
    pub fn exit_visual(&mut self) {
        self.visual = false;
        self.sync();
    }

    /// Toggle the mark on the focused pane's selected entry.
    pub fn toggle_selected(&mut self) {
        let pane = &self.levels[self.focus];
        let Some(entry) = pane.selected_entry() else {
            return;
        };
        let key = format!("{}/{}", pane.title, entry.name);
        if !self.marks.remove(&key) {
            self.marks.insert(key);
        }
        self.sync();
    }

    /// Drop every VISUAL mark (␣ c).
    pub fn clear_marks(&mut self) {
        self.marks.clear();
        self.sync();
    }

    /// Delete marked orgs from the orgs level (␣ d). Returns the
    /// deleted org names; non-org marks are left untouched (reported
    /// by the caller).
    pub fn delete_marked_orgs(&mut self) -> Vec<String> {
        let orgs_prefix = "orgs/";
        let deleted: Vec<String> = self
            .marks
            .iter()
            .filter_map(|k| k.strip_prefix(orgs_prefix).map(str::to_string))
            .collect();
        if deleted.is_empty() {
            return deleted;
        }
        self.levels[0]
            .entries
            .retain(|e| !deleted.contains(&e.name));
        for org in &deleted {
            self.marks.remove(&format!("{orgs_prefix}{org}"));
        }
        // Selected org deleted → drop to the first remaining entry.
        if let Some(sel) = self.levels[0].selected_entry()
            && deleted.contains(&sel.name)
        {
            self.levels[0].select(0);
        }
        self.sync();
        deleted
    }

    /// Marked entries as `"<pane title>/<name>"` keys.
    pub fn visual_marks(&self) -> Vec<String> {
        let mut marks: Vec<String> = self.marks.iter().cloned().collect();
        marks.sort();
        marks
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
    pub fn org_repos_loaded(&mut self, org: &str, repos: Vec<crate::provider::RepoInfo>) {
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

    fn current(&mut self) -> &mut Pane {
        &mut self.levels[self.focus]
    }

    /// Recompute focused flags + preview after any state change.
    fn sync(&mut self) {
        for (i, pane) in self.levels.iter_mut().enumerate() {
            pane.focused = i == self.focus;
            // Marks stay visible after leaving VISUAL (they drive
            // :clone / ␣d); ○ only while visual is active.
            let prefix = format!("{}/", pane.title);
            let pane_marks: std::collections::HashSet<String> = self
                .marks
                .iter()
                .filter_map(|k| k.strip_prefix(&prefix).map(str::to_string))
                .collect();
            pane.checkboxes = if self.visual || !pane_marks.is_empty() {
                Some(pane_marks)
            } else {
                None
            };
            pane.marks_only = !self.visual;
        }
        self.refresh_preview();
    }

    pub fn context(&self) -> String {
        // Path up to the focused level (level 0 = orgs, not shown).
        let crumbs = self
            .levels
            .iter()
            .skip(1)
            .take(self.focus)
            .map(|p| p.title.clone())
            .collect::<Vec<_>>()
            .join(" · ");
        // plans/0016 M1a: off-default revisions say so in the crumb.
        match &self.current_ref {
            Some(r) if !crumbs.is_empty() => format!("{crumbs} @ {r}"),
            _ => crumbs,
        }
    }

    pub fn selected_kind(&self) -> Option<EntryKind> {
        self.levels[self.focus].selected_entry().map(|e| e.kind)
    }

    /// Repos level title = owner of whatever is being browsed.
    fn current_owner(&self) -> Option<&str> {
        self.levels.get(1).map(|p| p.title.as_str())
    }

    /// Children of an entry: orgs never expand locally (API), repos/dirs
    /// expand from the loaded tree only — no mock trees, ever.
    fn children_of(&self, entry: &Entry) -> Option<Vec<Entry>> {
        let tree = self.tree.as_ref()?;
        match entry.kind {
            EntryKind::Org | EntryKind::File => None,
            EntryKind::Repo => (tree.owner == self.current_owner()? && tree.name == entry.name)
                .then(|| tree.children("")),
            EntryKind::Dir => {
                let base = self.dir_path();
                let child_path = if base.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{base}/{}", entry.name)
                };
                Some(tree.children(&child_path))
            }
        }
    }

    pub fn set_repo(&mut self, owner: &str, name: &str) {
        self.select_org(owner);
        // Reuse the API-loaded repos level if present; otherwise the
        // repos pane waits for the org load (never mock repos for an
        // org we haven't loaded — except the static defaults).
        if self.levels.len() < 2 || self.levels[1].title != owner {
            self.levels.truncate(1);
            self.levels.push(Pane::new(owner, vec![]));
        }
        if let Some(pos) = self.levels[1].entries.iter().position(|e| e.name == name) {
            self.levels[1].select(pos);
        } else {
            self.levels[1]
                .entries
                .push(Entry::new(name, EntryKind::Repo));
            let last = self.levels[1].entries.len() - 1;
            self.levels[1].select(last);
        }
        self.focus = 1;
        self.cascade();
        self.sync();
    }

    pub fn update(&mut self, action: &Action) -> Action {
        match action {
            Action::MoveUp | Action::MoveDown => {
                self.current().update(action);
                // Selection changed in the focused column: every column
                // to its right is stale — rebuild from the new selection.
                self.cascade();
                self.sync();
            }
            Action::DrillIn => {
                let action = self.drill_in();
                self.sync();
                return action;
            }
            Action::DrillOut => {
                if self.focus > 0 {
                    self.focus -= 1;
                }
                self.sync();
            }
            Action::PreviewLineDown => self.preview.move_cursor(1),
            Action::PreviewLineUp => self.preview.move_cursor(-1),
            _ => {}
        }
        Action::Noop
    }

    /// Drop levels right of focus and re-derive the immediate child
    /// level from the focused selection. Org entries never cascade —
    /// their repos come from the API (`LoadOrgRepos`).
    fn cascade(&mut self) {
        self.levels.truncate(self.focus + 1);
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            return;
        };
        if let Some(children) = self.children_of(&entry) {
            self.levels.push(Pane::new(entry.name.clone(), children));
        }
    }

    /// Drill into the focused entry. Org entries don't have their repos
    /// locally — the caller must fetch them (`LoadOrgRepos`); repos
    /// likewise fetch their tree (`LoadRepoTree`).
    fn drill_in(&mut self) -> Action {
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            return Action::Noop;
        };
        match entry.kind {
            EntryKind::File => Action::Noop, // OpenSelected handled by app
            EntryKind::Org => Action::LoadOrgRepos(entry.name.clone()),
            EntryKind::Repo => {
                if self.children_of(&entry).is_none() {
                    let owner = self.current_owner().unwrap_or_default().to_string();
                    return Action::LoadRepoTree {
                        owner,
                        name: entry.name,
                    };
                }
                if self.focus == self.levels.len() - 1
                    && let Some(children) = self.children_of(&entry)
                {
                    self.levels.push(Pane::new(entry.name.clone(), children));
                }
                if self.focus + 1 < self.levels.len() {
                    self.focus += 1;
                }
                Action::Noop
            }
            EntryKind::Dir => {
                if self.focus == self.levels.len() - 1
                    && let Some(children) = self.children_of(&entry)
                {
                    self.levels.push(Pane::new(entry.name.clone(), children));
                }
                if self.focus + 1 < self.levels.len() {
                    self.focus += 1;
                }
                Action::Noop
            }
        }
    }

    /// Dir path relative to the repo root, from the level titles
    /// (levels: 0=orgs, 1=repos, 2=repo root, 3+=dirs). Also persisted
    /// as `last_path` in the state store.
    pub fn dir_path(&self) -> String {
        self.levels
            .iter()
            .take(self.focus + 1)
            .skip(3)
            .map(|p| p.title.as_str())
            .collect::<Vec<_>>()
            .join("/")
    }

    pub fn apply_filter(&mut self) {
        let filter = self.filter_input.value();
        self.current().set_filter(filter);
        self.cascade();
        self.sync();
    }

    pub fn clear_filter(&mut self) {
        self.filter_input.clear();
        self.current().set_filter(String::new());
        self.cascade();
        self.sync();
    }

    /// The file under the cursor needs its blob fetched, if any.
    /// Taking a request marks the sha in-flight — without this, the
    /// end-of-route drain would re-request on every keystroke and
    /// recurse (stack overflow, caught by the filter-commit test).
    pub fn take_blob_request(&mut self) -> Option<(String, String)> {
        let (sha, name) = self.blob_request.take()?;
        self.pending_blobs
            .insert(sha.clone())
            .then_some((sha, name))
    }

    /// Re-highlight every cached blob under a new palette and refresh
    /// the visible preview (theme switched, plans/0007 §2).
    pub fn restyle_blobs(&mut self, highlighter: &crate::highlight::Highlighter) {
        for blob in self.blobs.values_mut() {
            blob.lines = highlighter.highlight(&blob.name, &blob.text);
        }
        self.refresh_preview();
    }

    /// Current preview line cursor (1-based) — anchors `␣ y` (v1.1).
    pub fn preview_line(&self) -> Option<u32> {
        self.preview.line()
    }

    /// The repo coordinates for a blob fetch.
    pub fn repo_coords(&self) -> Option<(String, String)> {
        let owner = self.current_owner()?.to_string();
        let name = self.tree.as_ref()?.name.clone();
        Some((owner, name))
    }

    /// The file under the cursor, as (full repo-relative path, blob sha).
    pub fn selected_file(&self) -> Option<(String, String)> {
        let entry = self.levels[self.focus].selected_entry()?;
        if entry.kind != EntryKind::File {
            return None;
        }
        let base = self.dir_path();
        let full = if base.is_empty() {
            entry.name.clone()
        } else {
            format!("{base}/{}", entry.name)
        };
        let sha = self.tree.as_ref()?.find(&full)?.sha.clone();
        Some((full, sha))
    }

    fn refresh_preview(&mut self) {
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            self.preview.content = Default::default();
            return;
        };
        match entry.kind {
            EntryKind::File => {
                let base = self.dir_path();
                let full = if base.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{base}/{}", entry.name)
                };
                let node = self.tree.as_ref().and_then(|t| t.find(&full)).cloned();
                match node {
                    Some(node) => {
                        if let Some(blob) = self.blobs.get(&node.sha) {
                            let lines = blob.lines.clone();
                            let lang = blob.lang.clone();
                            self.preview.set_highlighted(&entry.name, &lang, lines);
                        } else if let Some(err) = self.failed_blobs.get(&node.sha) {
                            // Re-selecting a failed fetch re-shows the
                            // honest error — never the loading
                            // placeholder again.
                            let err = err.clone();
                            self.preview
                                .set_error(&entry.name, node.size, &node.sha, &err);
                        } else {
                            if !self.pending_blobs.contains(&node.sha) {
                                self.blob_request = Some((node.sha.clone(), entry.name.clone()));
                            }
                            self.preview
                                .set_file_meta(&entry.name, node.size, &node.sha);
                        }
                    }
                    None => self.preview.content = Default::default(),
                }
            }
            EntryKind::Dir => {
                let children = self.children_of(&entry).unwrap_or_default();
                self.preview.set_dir(&entry.name, children);
            }
            EntryKind::Repo => {
                let children = self.children_of(&entry).unwrap_or_default();
                self.preview.set_dir(&entry.name, children);
            }
            EntryKind::Org => {
                // Repos load over the API; don't mock them in preview.
                self.preview.title = entry.name.clone();
                self.preview.content = Default::default();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme, zoomed: bool) {
        // ␣ p zoom (tmux `prefix z` model, plans/0016 M1): the preview
        // — or the history lens over it — takes the whole content row;
        // the miller columns are untouched underneath, Esc restores.
        if zoomed {
            if self.history.is_some() {
                self.render_history(frame, area, theme);
            } else {
                self.preview.render(frame, area, theme);
            }
            return;
        }
        // Top level (orgs): fold to a single full-width pane. There is
        // no parent above orgs, so a three-pane split would leave the
        // left column empty (PLAN.md §5).
        if self.focus == 0 {
            self.levels[0].render(frame, area, theme);
            return;
        }

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(40),
                Constraint::Percentage(40),
            ])
            .split(area);

        // Window over the level stack: parent | focused | preview.
        let parent = self.focus - 1;
        self.levels[parent].render(frame, cols[0], theme);
        let focus = self.focus;
        self.levels[focus].render(frame, cols[1], theme);
        // plans/0016 M1b: the history lens swaps the preview's content
        // (same rect, same border idiom) — the preview is untouched
        // underneath and Esc restores it.
        if self.history.is_some() {
            self.render_history(frame, cols[2], theme);
        } else {
            self.preview.render(frame, cols[2], theme);
        }
    }
}

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
