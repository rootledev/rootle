//! Clone wizard (`:clone`, plans/0004 §2): three screens — repo
//! checkboxes, local destination mini-browser, summary. Mock stage:
//! no git runs; the summary shows what *would* happen. Esc anywhere
//! closes the whole wizard (no partial state).
//!
//! Layout: this file is the state + public surface; `keys.rs` handles
//! input, `render.rs` draws.

use super::vim_input::VimInput;
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

pub struct CloneWizard {
    screen: Screen,
    /// Resolved repos (files already folded to their repo upstream).
    repos: Vec<(String, bool)>,
    cursor: usize,
    focus: Focus,
    /// Button row cursor: 0 = back, 1 = next/clone.
    button: usize,
    /// Destination browser state (screen 2).
    dest: PathBuf,
    dest_entries: Vec<String>,
    dest_cursor: usize,
    /// Line scroll offset of the current screen's content area.
    scroll: usize,
    /// Transient `/` filter (house style: every scanned list filters).
    filter: VimInput,
    filtering: bool,
    pre_filter: String,
    filter_value: String,
}

impl CloneWizard {
    pub fn new(repos: Vec<String>, start: PathBuf) -> Self {
        let repos = repos.into_iter().map(|r| (r, true)).collect();
        let mut wizard = CloneWizard {
            screen: Screen::Repos,
            repos,
            cursor: 0,
            focus: Focus::List,
            button: 1, // next is the default action
            dest: start,
            dest_entries: vec![],
            dest_cursor: 0,
            scroll: 0,
            filter: VimInput::transient(),
            filtering: false,
            pre_filter: String::new(),
            filter_value: String::new(),
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
        self.dest_cursor = 0;
    }

    fn checked(&self) -> impl Iterator<Item = &String> {
        self.repos.iter().filter(|(_, on)| *on).map(|(r, _)| r)
    }

    /// Repo indices surviving the committed filter.
    fn visible_repos(&self) -> Vec<usize> {
        let needle = self.filter_value.to_lowercase();
        self.repos
            .iter()
            .enumerate()
            .filter(|(_, (r, _))| needle.is_empty() || r.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    /// Destination indices surviving the committed filter.
    fn visible_dest(&self) -> Vec<usize> {
        let needle = self.filter_value.to_lowercase();
        self.dest_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| needle.is_empty() || e.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
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
