//! Three-pane miller browser over org → repo → dir → file (PLAN.md §1).
//! Yazi semantics: `h` moves focus left into the parent column, where
//! j/k then browse the parent — the child column rebuilds (cascades)
//! from the new selection. `l` drills back in / deeper.
//! Milestone 1: mock data only; GitHub backend lands in milestones 3–4.

use super::pane::{EntryKind, Pane};
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
        Self::new()
    }
}

impl Browser {
    pub fn new() -> Self {
        let mut browser = Browser {
            levels: vec![
                Pane::new("orgs", mock::orgs()),
                Pane::new("ratatui", mock::repos("ratatui")),
            ],
            focus: 1,
            preview: Preview::new(),
            filter_input: VimInput::transient(),
        };
        browser.sync();
        browser
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
        self.levels.truncate(1);
        self.levels.push(Pane::new(owner, mock::repos(owner)));
        self.levels.push(Pane::new(name, mock::dir(name, "")));
        self.focus = self.levels.len() - 1;
        self.sync();
    }

    pub fn update(&mut self, action: &Action) {
        match action {
            Action::MoveUp | Action::MoveDown => {
                self.current().update(action);
                // Selection changed in the focused column: every column
                // to its right is stale — rebuild from the new selection.
                self.cascade();
            }
            Action::DrillIn => self.drill_in(),
            Action::DrillOut => {
                if self.focus > 0 {
                    self.focus -= 1;
                }
            }
            _ => return,
        }
        self.sync();
    }

    /// Drop levels right of focus and re-derive the immediate child
    /// level from the focused selection (drillable entries only).
    fn cascade(&mut self) {
        self.levels.truncate(self.focus + 1);
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            return;
        };
        if let Some(children) = mock::children(&entry, &self.dir_path()) {
            self.levels.push(Pane::new(entry.name.clone(), children));
        }
    }

    fn drill_in(&mut self) {
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            return;
        };
        if entry.kind == EntryKind::File {
            return; // OpenSelected handled by app (editor, milestone 6)
        }
        if self.focus == self.levels.len() - 1 {
            // No child level yet (shouldn't happen post-cascade, but be safe).
            if let Some(children) = mock::children(&entry, &self.dir_path()) {
                self.levels.push(Pane::new(entry.name.clone(), children));
            }
        }
        if self.focus + 1 < self.levels.len() {
            self.focus += 1;
        }
    }

    /// Path of dir-level titles, for the mock/backend child lookup.
    fn dir_path(&self) -> String {
        self.levels
            .iter()
            .take(self.focus + 1)
            .skip(2)
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
            EntryKind::Dir | EntryKind::Repo | EntryKind::Org => {
                let children = mock::children(&entry, &self.dir_path()).unwrap_or_default();
                self.preview.set_dir(&entry.name, children);
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

    /// Children of an entry given the dir path of the level above it.
    /// None for files (not drillable).
    pub fn children(entry: &Entry, parent_path: &str) -> Option<Vec<Entry>> {
        match entry.kind {
            EntryKind::Org => Some(repos(&entry.name)),
            EntryKind::Repo => Some(dir(&entry.name, "")),
            EntryKind::Dir => Some(dir(&entry.name, parent_path)),
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
