//! Root: mode stack, action dispatch, component tree (PLAN.md §6).
//! GitHub calls run on worker threads (blocking reqwest); results return
//! over an mpsc channel as `AppEvent`s, drained once per event-loop tick.
//!
//! `App::with` constructs an **offline** app for tests: no workers are
//! spawned; backend outcomes are injected via `handle_action`.

use crate::action::Action;
use crate::components::browser::Browser;
use crate::components::modeline::Modeline;
use crate::components::pane::EntryKind;
use crate::components::search_popup::SearchPopup;
use crate::components::vim_input::Outcome;
use crate::config::Config;
use crate::event::{AppEvent, AppTx};
use crate::github::Client;
use crate::keymap;
use crate::mode::Mode;
use crate::state::State;
use crate::theme::Theme;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;
use std::sync::Arc;

pub struct App {
    mode: Mode,
    browser: Browser,
    popup: Option<SearchPopup>,
    modeline: Modeline,
    theme: Theme,
    #[allow(dead_code)]
    config: Config,
    state: State,
    tx: AppTx,
    client: Arc<Client>,
    /// Generation counter on search submissions; stale results dropped.
    search_gen: u64,
    /// One-line status shown in the modeline (searching/loading/error).
    status: Option<String>,
    /// Offline apps (tests) never spawn workers.
    offline: bool,
    pub should_quit: bool,
    /// Reserved for the editor-resume path (milestone 6): the only
    /// legitimate full `terminal.clear()` trigger (PLAN.md §9).
    pub force_redraw: bool,
}

impl App {
    pub fn new(tx: AppTx) -> Self {
        let mut app = Self::build(State::load(), tx, Client::new(), false);
        // Warm the repos level for the initially selected org.
        if let Some(org) = app.browser.selected_org() {
            app.handle_action(Action::LoadOrgRepos(org));
        }
        app
    }

    /// Offline, state-injectable constructor for tests.
    pub fn with(state: State, tx: AppTx) -> Self {
        Self::build(state, tx, Client::anonymous(), true)
    }

    fn build(state: State, tx: AppTx, client: Client, offline: bool) -> Self {
        // Resume flow: prefill the search with the last repo — one
        // Enter re-runs the query and returns to where the user was.
        let popup = SearchPopup::with_prefill(state.last_repo.as_deref());
        App {
            mode: Mode::Browse,
            browser: Browser::new(&state.recent_orgs),
            popup: Some(popup), // launch flow opens on search
            modeline: Modeline::new(),
            theme: Theme::catppuccin_mocha(),
            config: Config::load(),
            state,
            tx,
            client: Arc::new(client),
            search_gen: 0,
            status: None,
            offline,
            should_quit: false,
            force_redraw: false,
        }
    }

    fn effective_mode(&self) -> Mode {
        match &self.popup {
            Some(popup) => popup.effective_mode(),
            None => self.mode,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        let action = self.dispatch(key);
        self.handle_action(action);
    }

    /// Worker outcomes from the event channel.
    pub fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::SearchResults { gen, items } => {
                if gen != self.search_gen {
                    return; // stale submission
                }
                self.status = None;
                if let Some(popup) = &mut self.popup {
                    popup.update(&Action::SearchResults { items });
                }
            }
            AppEvent::SearchFailed { gen, message } => {
                if gen != self.search_gen {
                    return;
                }
                self.status = None;
                if let Some(popup) = &mut self.popup {
                    popup.update(&Action::SearchFailed { message });
                }
            }
            AppEvent::OrgReposLoaded { org, repos } => {
                self.status = None;
                self.browser.org_repos_loaded(&org, repos);
            }
            AppEvent::OrgReposFailed { org, message } => {
                self.status = Some(format!("{org}: {message}"));
            }
        }
    }

    fn dispatch(&mut self, key: KeyEvent) -> Action {
        if let Some(popup) = &mut self.popup {
            return popup.handle_key(key);
        }
        match self.mode {
            Mode::Browse => keymap::browsing(key.code),
            Mode::Search => match self.browser.filter_input.handle_key(key) {
                Outcome::Changed => Action::Noop, // filter applied below
                Outcome::Submitted => Action::CommitFilter,
                Outcome::Cancelled => Action::ClearFilter,
                Outcome::Noop => Action::Noop,
            },
            Mode::Leader => keymap::leader(key.code),
            _ => Action::Noop,
        }
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Quit | Action::LeaderQuit => {
                self.state.last_path = Some(self.browser.dir_path());
                self.state.save();
                self.should_quit = true;
            }
            Action::ClosePopup => {
                trace("ClosePopup");
                self.popup = None;
                self.mode = Mode::Browse;
            }
            Action::SearchSubmitted(_) => {
                self.search_gen += 1;
                self.status = Some("searching GitHub…".into());
                if let Some(popup) = &mut self.popup {
                    popup.update(&action);
                }
                if !self.offline {
                    self.spawn_search(self.search_gen);
                }
            }
            Action::SearchResults { .. } | Action::SearchFailed { .. } => {
                // Injected (tests) or misrouted worker outcomes.
                if let Some(popup) = &mut self.popup {
                    popup.update(&action);
                }
                self.status = None;
            }
            Action::RepoSelected { owner, name } => {
                trace(&format!("RepoSelected {owner}/{name}"));
                self.state.record_repo(&owner, &name);
                self.state.save();
                self.browser.set_repo(&owner, &name);
                self.popup = None;
                self.mode = Mode::Browse;
            }
            Action::OrgSelected(org) => {
                trace(&format!("OrgSelected {org}"));
                self.state.record_org(&org);
                self.state.save();
                self.browser.select_org(&org);
                self.popup = None;
                self.mode = Mode::Browse;
                self.handle_action(Action::LoadOrgRepos(org));
            }
            Action::LoadOrgRepos(org) => {
                self.status = Some(format!("loading {org}…"));
                if !self.offline {
                    self.spawn_org_repos(org);
                }
            }
            Action::OrgReposLoaded { org, repos } => {
                self.status = None;
                self.browser.org_repos_loaded(&org, repos);
            }
            Action::OrgReposFailed { org, message } => {
                self.status = Some(format!("{org}: {message}"));
            }
            Action::Leader => self.mode = Mode::Leader,
            Action::LeaderSearch => {
                self.popup = Some(SearchPopup::new());
                self.mode = Mode::Browse;
            }
            Action::EnterSearch => {
                self.browser.filter_input.submode = crate::components::vim_input::SubMode::Insert;
                self.mode = Mode::Search;
            }
            Action::CommitFilter => self.mode = Mode::Browse,
            Action::ClearFilter => {
                self.browser.clear_filter();
                self.mode = Mode::Browse;
            }
            Action::MoveUp | Action::MoveDown | Action::DrillIn | Action::DrillOut => {
                let follow = self.browser.update(&action);
                self.handle_action(follow);
            }
            Action::OpenSelected => {
                if matches!(
                    self.browser.selected_kind(),
                    Some(EntryKind::Dir | EntryKind::Repo | EntryKind::Org)
                ) {
                    let follow = self.browser.update(&Action::DrillIn);
                    self.handle_action(follow);
                }
                // Files: editor integration is milestone 6.
            }
            Action::Noop => {}
        }

        // Incremental filter: re-apply on every SEARCH keystroke.
        if self.mode == Mode::Search {
            self.browser.apply_filter();
        }
    }

    fn spawn_search(&self, gen: u64) {
        let Some(popup) = &self.popup else { return };
        let query = popup.input.value();
        let client = self.client.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            trace(&format!("search start gen={gen} q={query:?}"));
            let event = match client.search(&query) {
                Ok(items) => {
                    trace(&format!("search ok gen={gen} items={}", items.len()));
                    AppEvent::SearchResults { gen, items }
                }
                Err(message) => {
                    trace(&format!("search ERR gen={gen} {message}"));
                    AppEvent::SearchFailed { gen, message }
                }
            };
            let _ = tx.send(event);
            trace(&format!("search sent gen={gen}"));
        });
    }

    fn spawn_org_repos(&self, org: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match client.org_repos(&org) {
                Ok(repos) => AppEvent::OrgReposLoaded { org, repos },
                Err(message) => AppEvent::OrgReposFailed { org, message },
            };
            let _ = tx.send(event);
        });
    }

    /// Desired terminal cursor shape, if any text input is focused.
    pub fn cursor_style(&self) -> Option<ratatui::crossterm::cursor::SetCursorStyle> {
        self.popup.as_ref().and_then(|p| p.cursor_style())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        self.browser.render(frame, rows[0], &self.theme);
        self.modeline.context = self.browser.context();
        self.modeline.status = self.status.clone();
        self.modeline
            .render(frame, rows[1], self.effective_mode(), &self.theme);

        if let Some(popup) = &mut self.popup {
            popup.render(frame, rows[0], &self.theme);
        }
    }
}
/// Worker debug tracing: enabled via GHX_TRACE=/path/log (kept minimal,
/// no logging dependency; remove when the backend stabilizes).
fn trace(msg: &str) {
    if let Ok(path) = std::env::var("GHX_TRACE") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{:?} {msg}", std::time::SystemTime::now());
        }
    }
}
