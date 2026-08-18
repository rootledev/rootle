//! Three-pane miller browser over org → repo → dir → file (PLAN.md §1).
//! Yazi semantics: `h` moves focus left into the parent column, where
//! j/k then browse the parent — the child column rebuilds (cascades)
//! from the new selection. `l` drills back in / deeper.
//! Org repos arrive asynchronously from the GitHub API; repo/dir trees
//! are still mock data until milestone 4.

use super::pane::{Entry, EntryKind, Pane};
use super::preview::Preview;
use super::vim_input::VimInput;
use crate::action::Action;
use crate::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

pub struct Browser {
    /// Level stack: levels[i] lists the children of the selection in
    /// levels[i-1]. levels[0] = orgs, levels[1] = repos, then dirs.
    levels: Vec<Pane>,
    /// Which level owns the keyboard. Default = deepest level.
    focus: usize,
    pub preview: Preview,
    /// `/` filter input, owned here, active in SEARCH mode.
    pub filter_input: VimInput,
}

impl Default for Browser {
    fn default() -> Self {
        Self::new(&[])
    }
}

impl Browser {
    pub fn new(recent_orgs: &[String]) -> Self {
        let mut names: Vec<String> = recent_orgs.to_vec();
        for d in ["ratatui", "tokio-rs", "helix-editor"] {
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
            preview: Preview::new(),
            filter_input: VimInput::transient(),
        };
        browser.sync();
        browser
    }

    /// Selected org at the top level, if any.
    pub fn selected_org(&self) -> Option<String> {
        self.levels[0].selected_entry().map(|e| e.name.clone())
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
    pub fn org_repos_loaded(&mut self, org: &str, repos: Vec<String>) {
        if self.selected_org().as_deref() != Some(org) {
            return;
        }
        let entries = repos
            .iter()
            .map(|r| Entry::new(r, EntryKind::Repo))
            .collect();
        self.levels.truncate(1);
        self.levels.push(Pane::new(org, entries));
        self.focus = 1;
        self.cascade();
        self.sync();
    }

    fn current(&mut self) -> &mut Pane {
        &mut self.levels[self.focus]
    }

    /// Recompute focused flags + preview after any state change.
    fn sync(&mut self) {
        for (i, pane) in self.levels.iter_mut().enumerate() {
            pane.focused = i == self.focus;
        }
        self.refresh_preview();
    }

    pub fn context(&self) -> String {
        // Path up to the focused level (level 0 = orgs, not shown).
        self.levels
            .iter()
            .skip(1)
            .take(self.focus)
            .map(|p| p.title.clone())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn selected_kind(&self) -> Option<EntryKind> {
        self.levels[self.focus].selected_entry().map(|e| e.kind)
    }

    pub fn set_repo(&mut self, owner: &str, name: &str) {
        self.select_org(owner);
        // Reuse the API-loaded repos level if present; mock is the
        // fallback until the org load lands (milestone 4 replaces trees).
        if self.levels.len() < 2 || self.levels[1].title != owner {
            self.levels.truncate(1);
            self.levels.push(Pane::new(owner, mock::repos(owner)));
        }
        if let Some(pos) = self.levels[1].entries.iter().position(|e| e.name == name) {
            self.levels[1].select(pos);
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
            _ => {}
        }
        Action::Noop
    }

    /// Drill into the focused entry. Org entries don't have their repos
    /// locally — the caller must fetch them (`LoadOrgRepos`).
    fn drill_in(&mut self) -> Action {
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            return Action::Noop;
        };
        match entry.kind {
            EntryKind::File => Action::Noop, // OpenSelected handled by app
            EntryKind::Org => Action::LoadOrgRepos(entry.name.clone()),
            EntryKind::Repo | EntryKind::Dir => {
                if self.focus == self.levels.len() - 1 {
                    if let Some(children) = mock::children(&entry, &self.dir_path()) {
                        self.levels.push(Pane::new(entry.name.clone(), children));
                    }
                }
                if self.focus + 1 < self.levels.len() {
                    self.focus += 1;
                }
                Action::Noop
            }
        }
    }

    /// Drop levels right of focus and re-derive the immediate child
    /// level from the focused selection. Org entries never cascade —
    /// their repos come from the API (`LoadOrgRepos`).
    fn cascade(&mut self) {
        self.levels.truncate(self.focus + 1);
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            return;
        };
        if entry.kind == EntryKind::Org {
            return;
        }
        if let Some(children) = mock::children(&entry, &self.dir_path()) {
            self.levels.push(Pane::new(entry.name.clone(), children));
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

    fn refresh_preview(&mut self) {
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            self.preview.content = Default::default();
            return;
        };
        match entry.kind {
            EntryKind::File => self
                .preview
                .set_bytes(&entry.name, &mock::file_bytes(&entry.name)),
            EntryKind::Dir | EntryKind::Repo => {
                let children = mock::children(&entry, &self.dir_path()).unwrap_or_default();
                self.preview.set_dir(&entry.name, children);
            }
            EntryKind::Org => {
                // Repos load over the API; don't mock them in preview.
                self.preview.title = entry.name.clone();
                self.preview.content = Default::default();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
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
        self.preview.render(frame, cols[2], theme);
    }
}

/// Mock data — replaced by the GitHub backend (milestones 3–4).
pub mod mock {
    use super::super::pane::{Entry, EntryKind};

    pub fn orgs() -> Vec<Entry> {
        vec![
            Entry::new("ratatui", EntryKind::Org),
            Entry::new("tokio-rs", EntryKind::Org),
            Entry::new("helix-editor", EntryKind::Org),
        ]
    }

    pub fn repos(org: &str) -> Vec<Entry> {
        let names: &[&str] = match org {
            "ratatui" => &["ratatui", "ratatui-website", "templates", "comfy-table"],
            "tokio-rs" => &["tokio", "axum", "hyper", "tracing", "bytes"],
            "helix-editor" => &["helix", "helix-term"],
            _ => &["ratatui"],
        };
        names
            .iter()
            .map(|n| Entry::new(n, EntryKind::Repo))
            .collect()
    }

    pub fn dir(_repo: &str, path: &str) -> Vec<Entry> {
        let entries: &[(&str, EntryKind)] = match path {
            "" => &[
                ("src", EntryKind::Dir),
                ("docs", EntryKind::Dir),
                ("examples", EntryKind::Dir),
                ("Cargo.toml", EntryKind::File),
                ("README.md", EntryKind::File),
                ("LICENSE", EntryKind::File),
            ],
            "src" => &[
                ("widgets", EntryKind::Dir),
                ("layout", EntryKind::Dir),
                ("lib.rs", EntryKind::File),
                ("terminal.rs", EntryKind::File),
                ("malformed.bin", EntryKind::File),
            ],
            _ => &[
                ("mod.rs", EntryKind::File),
                ("block.rs", EntryKind::File),
                ("paragraph.rs", EntryKind::File),
            ],
        };
        entries.iter().map(|(n, k)| Entry::new(n, *k)).collect()
    }

    /// Children of an entry given the dir path of the level it sits in.
    /// None for files (not drillable).
    pub fn children(entry: &Entry, parent_path: &str) -> Option<Vec<Entry>> {
        match entry.kind {
            EntryKind::Org => Some(repos(&entry.name)),
            EntryKind::Repo => Some(dir(&entry.name, "")),
            EntryKind::Dir => {
                let child_path = if parent_path.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{parent_path}/{}", entry.name)
                };
                Some(dir(&entry.name, &child_path))
            }
            EntryKind::File => None,
        }
    }

    /// Mock file bytes. `malformed.bin` deliberately contains ESC/control
    /// bytes to exercise the sanitization boundary (PLAN.md §9).
    pub fn file_bytes(name: &str) -> Vec<u8> {
        match name {
            "malformed.bin" => b"\x1b[2J\x1b[H wiped?\x07\x00 binary-ish".to_vec(),
            "lib.rs" => b"//! A Rust TUI library.\n\npub mod terminal;\npub mod widgets;\n\n\x1b]8;;evil\x1b\\link\x07 stripped by sanitize\n"
                .to_vec(),
            "Cargo.toml" => b"[package]\nname = \"ratatui\"\nversion = \"0.29.0\"\n".to_vec(),
            "README.md" => b"# ratatui\n\nA Rust crate for cooking up terminal user interfaces.\n"
                .to_vec(),
            _ => b"// mock content\n".to_vec(),
        }
    }
}
