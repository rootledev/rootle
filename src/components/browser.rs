//! Three-pane miller browser over org → repo → dir → file (PLAN.md §1).
//! Milestone 1: mock data only; GitHub backend lands in milestones 3–4.

use super::pane::{EntryKind, Pane};
use super::preview::Preview;
use super::vim_input::VimInput;
use crate::action::Action;
use crate::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

pub struct Browser {
    /// Level stack; last = current (center column).
    levels: Vec<Pane>,
    pub preview: Preview,
    /// `/` filter input, owned here, active in SEARCHING mode.
    pub filter_input: VimInput,
}

impl Browser {
    pub fn new() -> Self {
        let orgs = mock::orgs();
        let mut browser = Browser {
            levels: vec![
                Pane::new("orgs", orgs),
                Pane::new("ratatui", mock::repos("ratatui")),
            ],
            preview: Preview::new(),
            filter_input: VimInput::new(),
        };
        browser.focus_current();
        browser.refresh_preview();
        browser
    }

    fn current(&mut self) -> &mut Pane {
        self.levels.last_mut().expect("level stack never empty")
    }

    fn focus_current(&mut self) {
        let last = self.levels.len() - 1;
        for (i, pane) in self.levels.iter_mut().enumerate() {
            pane.focused = i == last;
        }
    }

    pub fn context(&self) -> String {
        self.levels
            .iter()
            .skip(1)
            .map(|p| p.title.clone())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn selected_kind(&self) -> Option<EntryKind> {
        self.levels
            .last()
            .and_then(|p| p.selected_entry())
            .map(|e| e.kind)
    }

    pub fn set_repo(&mut self, owner: &str, name: &str) {
        self.levels.truncate(1);
        self.levels.push(Pane::new(owner, mock::repos(owner)));
        self.levels.push(Pane::new(name, mock::dir(name, "")));
        self.focus_current();
        self.refresh_preview();
    }

    pub fn update(&mut self, action: &Action) -> Action {
        match action {
            Action::MoveUp | Action::MoveDown => {
                self.current().update(action);
                self.refresh_preview();
            }
            Action::DrillIn => self.drill_in(),
            Action::DrillOut => {
                if self.levels.len() > 2 {
                    self.levels.pop();
                    self.focus_current();
                    self.refresh_preview();
                }
            }
            _ => {}
        }
        Action::Noop
    }

    fn drill_in(&mut self) {
        let Some(entry) = self.current().selected_entry().cloned() else {
            return;
        };
        let title = entry.name.clone();
        let children = match entry.kind {
            EntryKind::Org => mock::repos(&title),
            EntryKind::Repo => mock::dir(&title, ""),
            EntryKind::Dir => {
                let path = self
                    .levels
                    .iter()
                    .skip(2)
                    .map(|p| p.title.as_str())
                    .collect::<Vec<_>>()
                    .join("/");
                mock::dir(&title, &path)
            }
            EntryKind::File => return, // OpenSelected handled by app
        };
        self.levels.push(Pane::new(title, children));
        self.focus_current();
        self.refresh_preview();
    }

    pub fn apply_filter(&mut self) {
        let filter = self.filter_input.value();
        self.current().set_filter(filter);
        self.refresh_preview();
    }

    pub fn clear_filter(&mut self) {
        self.filter_input.clear();
        self.current().set_filter(String::new());
        self.refresh_preview();
    }

    fn refresh_preview(&mut self) {
        let Some(entry) = self.current().selected_entry().cloned() else {
            self.preview.content = Default::default();
            return;
        };
        match entry.kind {
            EntryKind::File => self.preview.set_bytes(&entry.name, &mock::file_bytes(&entry.name)),
            EntryKind::Dir | EntryKind::Repo | EntryKind::Org => {
                let children = mock::children_names(&entry);
                self.preview.set_dir(&entry.name, children);
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(40),
                Constraint::Percentage(40),
            ])
            .split(area);

        // Parent column: previous level, or empty placeholder at top.
        let len = self.levels.len();
        if len >= 2 {
            let idx = len - 2;
            self.levels[idx].render(frame, cols[0], theme);
        } else {
            Pane::new("", vec![]).render(frame, cols[0], theme);
        }
        let current = self.levels.len() - 1;
        self.levels[current].render(frame, cols[1], theme);
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
        names.iter().map(|n| Entry::new(n, EntryKind::Repo)).collect()
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

    pub fn children_names(entry: &Entry) -> Vec<String> {
        let entries = match entry.kind {
            EntryKind::Org => repos(&entry.name),
            EntryKind::Repo => dir(&entry.name, ""),
            _ => dir("", "other"),
        };
        entries
            .iter()
            .map(|e| match e.kind {
                EntryKind::File => e.name.clone(),
                _ => format!("{}/", e.name),
            })
            .collect()
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
