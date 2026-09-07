//! Clone wizard: repository checklist, destination browser, then summary.
//! State and row data live here; input/rendering are sibling modules.

use super::list_view::{ListCursor, ListFilter, Viewport};
use rootle_provider::RepoInfo;
use std::path::PathBuf;

mod keys;
mod render;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Repos,
    Destination,
    Summary,
}

/// List or the button row owns the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Buttons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Back,
    Next,
}

pub struct CloneWizard {
    screen: Screen,
    /// Resolved repos (files already folded to their repo upstream),
    /// with v1.4 listing metadata when the provider reported it
    /// (plans/0014 #1): recently pushed first, undated by name.
    repos: Vec<(RepoInfo, bool)>,
    selection: ListCursor,
    focus: Focus,
    button: Button,
    /// Destination browser state (screen 2).
    dest: PathBuf,
    dest_entries: Vec<String>,
    destination_selection: ListCursor,
    viewport: Viewport,
    filter: ListFilter,
}

impl CloneWizard {
    pub(crate) fn diagnostics(&self, full: bool) -> serde_json::Value {
        serde_json::json!({
            "screen":match self.screen { Screen::Repos => "repos", Screen::Destination => "destination", Screen::Summary => "summary" },
            "focus":match self.focus { Focus::List => "list", Focus::Buttons => "buttons" },
            "button":match self.button { Button::Back => "back", Button::Next => "next" },
            "selected":self.selection.selected().get(),
            "destination_selected":self.destination_selection.selected().get(),
            "destination":self.dest.to_string_lossy(), "repositories":self.repos.len(),
            "marked":self.repos.iter().filter(|(_,marked)|*marked).count(),
            "directories":self.dest_entries.len(), "viewport":self.viewport.diagnostics(),
            "filter":self.filter.diagnostics(full),
        })
    }

    pub fn effective_mode(&self) -> crate::mode::Mode {
        if self.filter.active() {
            crate::mode::Mode::Search
        } else {
            crate::mode::Mode::Browse
        }
    }
    pub fn new(repos: Vec<RepoInfo>, start: PathBuf) -> Self {
        // v1.4 (plans/0014 #1): sort by pushed_at desc — ISO-8601
        // sorts lexicographically; undated entries fall back to name
        // order (providers that send bare names keep the old order).
        let mut repos = repos;
        repos.sort_by(|a, b| {
            b.pushed_at
                .cmp(&a.pushed_at)
                .then_with(|| a.name.cmp(&b.name))
        });
        let repos = repos.into_iter().map(|r| (r, true)).collect();
        let mut wizard = CloneWizard {
            screen: Screen::Repos,
            repos,
            selection: ListCursor::new(),
            focus: Focus::List,
            button: Button::Next,
            dest: start,
            dest_entries: vec![],
            destination_selection: ListCursor::new(),
            viewport: Viewport::default(),
            filter: ListFilter::default(),
        };
        wizard.refresh_dest();
        wizard
    }

    /// Local dirs of the current destination path, `..` first.
    fn refresh_dest(&mut self) {
        let mut entries = vec!["..".to_string()];
        if let Ok(read) = std::fs::read_dir(&self.dest) {
            let mut dirs: Vec<String> = read
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| !name.starts_with('.')) // no dotdirs
                .collect();
            dirs.sort();
            entries.extend(dirs);
        }
        self.dest_entries = entries;
        self.destination_selection.reset();
        self.viewport.reset();
        self.filter.clear();
    }

    fn checked(&self) -> impl Iterator<Item = &RepoInfo> {
        self.repos.iter().filter(|(_, on)| *on).map(|(r, _)| r)
    }

    /// Repo indices surviving the committed filter.
    fn visible_repos(&self) -> Vec<usize> {
        self.filter.visible(&self.repos, |(repository, _), filter| {
            filter.matches(&repository.name)
        })
    }

    /// Destination indices surviving the committed filter.
    fn visible_dest(&self) -> Vec<usize> {
        self.filter
            .visible(&self.dest_entries, |entry, filter| filter.matches(entry))
    }

    /// Clone target: <dest>/<org>/<repo> — the org level prevents
    /// same-name collisions between orgs.
    fn target(&self, repo: &str) -> std::path::PathBuf {
        self.dest.join(repo)
    }

    /// Button labels: arrows mark direction of travel (`← Back`,
    /// `Next →` — the standard wizard convention) and the committing
    /// action names its key (`⏎ Clone`). The glyphs ship in both the
    /// vendored Nerd Font Mono and plain JetBrains Mono.
    fn buttons(&self) -> (&'static str, &'static str) {
        match self.screen {
            Screen::Repos | Screen::Destination => ("← Back", "Next →"),
            Screen::Summary => ("← Back", "⏎ Clone"),
        }
    }
}
