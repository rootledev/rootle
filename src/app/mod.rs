//! Root: mode stack, action dispatch, component tree (PLAN.md §6).
//! GitHub calls run on worker threads (blocking reqwest); results return
//! over an mpsc channel as `AppEvent`s, drained once per event-loop tick.
//!
//! `App::with` constructs an **offline** app for tests: no workers are
//! spawned; backend outcomes are injected via `handle_action`.

use crate::action::Action;
use crate::components::browser::Browser;
use crate::components::clone_wizard::CloneWizard;
use crate::components::command_line::CommandLine;
use crate::components::global_search::{GlobalSearch, SearchKind};
use crate::components::keybinds_popup::KeybindsPopup;
use crate::components::modeline::Modeline;
use crate::components::pane::EntryKind;
use crate::components::search_popup::SearchPopup;
use crate::components::settings_popup::SettingsPopup;
use crate::components::vim_input::Outcome;
use crate::config::Config;
use crate::event::{AppEvent, AppTx};
use crate::highlight::Highlighter;
use crate::keymap;
use crate::mode::Mode;
use crate::provider::{self, Provider};
use crate::state::State;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use std::sync::Arc;

mod workers;

pub struct App {
    mode: Mode,
    browser: Browser,
    popup: Option<SearchPopup>,
    /// Full-screen global search view (␣ f / ␣ g); replaces the
    /// browser while open (plans/0002-v0.2).
    search_view: Option<GlobalSearch>,
    /// Overlays (plans/0003/0004): at most one at a time.
    help: Option<KeybindsPopup>,
    command_line: Option<CommandLine>,
    settings: Option<SettingsPopup>,
    wizard: Option<CloneWizard>,
    modeline: Modeline,
    theme: Theme,
    config: Config,
    state: State,
    tx: AppTx,
    provider: Arc<dyn Provider>,
    highlighter: Highlighter,
    /// Generation counter on search submissions; stale results dropped.
    search_gen: u64,
    /// Same for the global search view (find/grep workers).
    view_gen: u64,
    /// One-line status shown in the modeline (searching/loading/error).
    status: Option<String>,
    /// Offline apps (tests) never spawn workers.
    offline: bool,
    pub should_quit: bool,
    /// Reserved for the editor-resume path (milestone 6): the only
    /// legitimate full `terminal.clear()` trigger (PLAN.md §9).
    pub force_redraw: bool,
    /// A prepared editor invocation; the main loop runs it while the
    /// terminal is suspended, then forces a full redraw.
    pending_editor: Option<crate::editor::EditorJob>,
    /// Queued yank (␣ y): the main loop writes it to the clipboard
    /// outside the draw path (plans/0003 §1).
    pending_clipboard: Option<String>,
}

impl App {
    pub fn new(tx: AppTx, config: Config, theme: Theme) -> Self {
        let (provider, warning) = provider::build(&config);
        let mut app = Self::build(State::load(), tx, provider, false, config, theme);
        if let Some(warning) = warning {
            app.status = Some(warning);
        }
        // Warm the repos level for the initially selected org.
        if let Some(org) = app.browser.selected_org() {
            app.handle_action(Action::LoadOrgRepos(org));
        }
        app
    }

    /// Offline, state-injectable constructor for tests.
    pub fn with(state: State, tx: AppTx) -> Self {
        Self::build(
            state,
            tx,
            provider::offline(),
            true,
            Config::default(),
            Theme::catppuccin_mocha(),
        )
    }

    fn build(
        state: State,
        tx: AppTx,
        provider: Arc<dyn Provider>,
        offline: bool,
        config: Config,
        theme: Theme,
    ) -> Self {
        // Launch flow: the repo search popup opens automatically only
        // for a fresh install (no repos in state yet). With recents,
        // the browser opens directly; ␣ s still offers resume via the
        // prefilled last repo.
        let popup = if state.recent_repos.is_empty()
            && state.recent_orgs.is_empty()
            && state.last_repo.is_none()
        {
            Some(SearchPopup::with_prefill(state.last_repo.as_deref()))
        } else {
            None
        };
        App {
            mode: Mode::Browse,
            browser: Browser::new(&state.recent_orgs, &provider.default_orgs()),
            popup, // opens on launch only for a fresh state
            search_view: None,
            help: None,
            command_line: None,
            settings: None,
            wizard: None,
            modeline: Modeline::new(),
            theme,
            config,
            state,
            tx,
            provider,
            highlighter: Highlighter::new(),
            search_gen: 0,
            view_gen: 0,
            status: None,
            offline,
            should_quit: false,
            force_redraw: false,
            pending_editor: None,
            pending_clipboard: None,
        }
    }

    fn effective_mode(&self) -> Mode {
        if self.command_line.is_some() {
            return Mode::Insert;
        }
        if let Some(settings) = &self.settings {
            return settings.effective_mode();
        }
        if self.help.is_some() || self.wizard.is_some() {
            return Mode::Browse;
        }
        if let Some(view) = &self.search_view {
            // The leader layer can be raised over the view (␣ from the
            // results) — it owns the keys and the modeline while up.
            if self.mode == Mode::Leader {
                return Mode::Leader;
            }
            return view.effective_mode();
        }
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
            AppEvent::SearchResults { gen_id, items } => {
                if gen_id != self.search_gen {
                    return; // stale submission
                }
                self.status = None;
                if let Some(popup) = &mut self.popup {
                    popup.update(&Action::SearchResults { items });
                }
            }
            AppEvent::SearchFailed { gen_id, message } => {
                if gen_id != self.search_gen {
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
            AppEvent::TreeLoaded {
                owner,
                name,
                entries,
                truncated,
                branch,
            } => {
                self.handle_action(Action::TreeLoaded {
                    owner,
                    name,
                    entries,
                    truncated,
                    branch,
                });
            }
            AppEvent::BlobLoaded { sha, name, bytes } => {
                self.handle_action(Action::BlobLoaded { sha, name, bytes });
            }
            AppEvent::BlobFailed { sha, message } => {
                self.handle_action(Action::BlobFailed { sha, message });
            }
            AppEvent::TreeFailed {
                owner,
                name,
                message,
            } => {
                self.handle_action(Action::TreeFailed {
                    owner,
                    name,
                    message,
                });
            }
            AppEvent::GlobalSearchResults { gen_id, hits } => {
                if gen_id != self.view_gen {
                    return; // stale submission
                }
                self.status = None;
                let Some(view) = &self.search_view else {
                    return;
                };
                let (kind, query) = (view.kind(), view.query.value());
                let hits = hits
                    .into_iter()
                    .map(crate::components::global_search::SearchHit::from_raw)
                    .collect();
                let hits = self.finish_hits(hits, kind, &query);
                if let Some(view) = &mut self.search_view {
                    view.update(&Action::GlobalSearchResults { hits });
                }
            }
            AppEvent::GlobalSearchFailed { gen_id, message } => {
                if gen_id != self.view_gen {
                    return;
                }
                self.status = None;
                if let Some(view) = &mut self.search_view {
                    view.update(&Action::GlobalSearchFailed { message });
                }
            }
            AppEvent::CloneExpanded { repos, errors } => {
                if repos.is_empty() {
                    self.status = Some(if errors.is_empty() {
                        "nothing to clone".into()
                    } else {
                        format!("no repos: {}", errors.join("; "))
                    });
                } else {
                    if !errors.is_empty() {
                        self.status = Some(format!("some orgs failed: {}", errors.join("; ")));
                    }
                    let cwd = std::env::current_dir().unwrap_or_default();
                    self.wizard = Some(CloneWizard::new(repos, cwd));
                }
            }
            AppEvent::CloneDone { ok, failed } => {
                let mut status = format!(
                    "cloned {} repo{}",
                    ok.len(),
                    if ok.len() == 1 { "" } else { "s" }
                );
                if !failed.is_empty() {
                    status.push_str(&format!(
                        ", {} failed ({} …)",
                        failed.len(),
                        failed[0].1.chars().take(40).collect::<String>()
                    ));
                }
                self.status = Some(status);
            }
        }
    }

    fn dispatch(&mut self, key: KeyEvent) -> Action {
        // Overlays capture keys, topmost first.
        if let Some(wizard) = &mut self.wizard {
            return wizard.handle_key(key);
        }
        if let Some(settings) = &mut self.settings {
            return settings.handle_key(key);
        }
        if let Some(help) = &mut self.help {
            return help.handle_key(key);
        }
        if let Some(command_line) = &mut self.command_line {
            return command_line.handle_key(key);
        }
        if let Some(view) = &mut self.search_view {
            // While the leader layer is up, it owns the keys — the
            // view regains them on the action that follows.
            if self.mode == Mode::Leader {
                return keymap::leader(key.code);
            }
            return view.handle_key(key);
        }
        if let Some(popup) = &mut self.popup {
            return popup.handle_key(key);
        }
        match self.mode {
            Mode::Browse => keymap::browsing(key.code),
            Mode::Visual => keymap::visual(key.code),
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
                // Topmost overlay first; the search popup last.
                if self.wizard.take().is_some()
                    || self.settings.take().is_some()
                    || self.help.take().is_some()
                    || self.command_line.take().is_some()
                {
                    return;
                }
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
                self.handle_action(Action::LoadRepoTree { owner, name });
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
            Action::LoadRepoTree { owner, name } => {
                self.status = Some(format!("loading {owner}/{name} tree…"));
                if !self.offline {
                    self.spawn_tree(owner, name);
                }
            }
            Action::TreeLoaded {
                owner,
                name,
                entries,
                truncated,
                branch,
            } => {
                self.status = None;
                self.browser
                    .tree_loaded(&owner, &name, entries, truncated, branch);
            }
            Action::TreeFailed {
                owner,
                name,
                message,
            } => {
                self.status = Some(format!("{owner}/{name}: {message}"));
            }
            Action::LoadBlob { sha, name } => {
                if !self.offline {
                    self.spawn_blob(sha, name);
                }
            }
            Action::BlobLoaded { sha, name, bytes } => {
                // Sanitize at the boundary, highlight once, cache in
                // the browser (PLAN.md §9).
                if crate::sanitize::is_binary(&bytes) {
                    self.browser.blob_failed(&sha, "binary file");
                    return;
                }
                let text = crate::sanitize::sanitize(&bytes);
                let lines = self.highlighter.highlight(&name, &text);
                self.browser.blob_loaded(&sha, lines);
            }
            Action::BlobFailed { sha, message } => {
                self.browser.blob_failed(&sha, &message);
            }
            Action::Leader => self.mode = Mode::Leader,
            Action::KeybindsPopup => self.help = Some(KeybindsPopup::new()),
            Action::CommandLine => self.command_line = Some(CommandLine::new()),
            Action::RunCommand(name) => {
                self.command_line = None;
                match name.as_str() {
                    "settings" => {
                        let themes = Theme::available_names();
                        self.settings = Some(SettingsPopup::new(&self.config, themes));
                    }
                    "clone" => {
                        let (repos, orgs) = self.clone_candidates();
                        if orgs.is_empty() {
                            let cwd = std::env::current_dir().unwrap_or_default();
                            self.wizard = Some(CloneWizard::new(repos, cwd));
                        } else {
                            self.status = Some(format!("expanding {} org(s)…", orgs.len()));
                            if !self.offline {
                                self.spawn_expand_clone(repos, orgs);
                            }
                        }
                    }
                    other => self.status = Some(format!("unknown command: {other}")),
                }
            }
            Action::Visual => {
                self.mode = Mode::Visual;
                self.browser.enter_visual();
            }
            Action::ExitVisual => {
                self.mode = Mode::Browse;
                self.browser.exit_visual();
            }
            Action::ToggleSelect => self.browser.toggle_selected(),
            Action::LeaderReload => {
                self.mode = Mode::Browse;
                if let Some((owner, name)) = self.browser.repo_coords() {
                    // Conditional refetch: cheap when the ref ETag is
                    // still fresh (304), fresh tree when it moved.
                    self.handle_action(Action::LoadRepoTree { owner, name });
                    self.status = Some("reloading tree…".into());
                } else if let Some(org) = self.browser.selected_org() {
                    self.handle_action(Action::LoadOrgRepos(org));
                    self.status = Some("reloading org repos…".into());
                } else {
                    self.status = Some("nothing to reload".into());
                }
            }
            Action::DeleteMarked => {
                self.mode = Mode::Browse;
                let deleted = self.browser.delete_marked_orgs();
                if deleted.is_empty() {
                    self.status = Some("no marked orgs (mark orgs in VISUAL, ␣d)".into());
                } else {
                    // Keep persisted recents in sync.
                    self.state.recent_orgs.retain(|o| !deleted.contains(o));
                    if self
                        .state
                        .last_org
                        .as_deref()
                        .is_some_and(|o| deleted.iter().any(|d| d == o))
                    {
                        self.state.last_org = None;
                    }
                    self.state.save();
                    self.browser.clear_marks();
                    self.status = Some(format!("deleted {} org(s)", deleted.len()));
                }
            }
            Action::ClearMarks => {
                self.mode = Mode::Browse;
                self.browser.clear_marks();
                self.status = Some("marks cleared".into());
            }
            Action::LeaderYank => {
                // Mock stage (plans/0003 §1): toast the URL that would
                // be yanked; clipboard (OSC 52) wires up later.
                self.mode = Mode::Browse;
                // URLs come from the provider — no GitHub grammar
                // outside the GitHub impl (plans/0005).
                let url = if let Some(view) = &self.search_view {
                    view.selected_hit().and_then(|h| {
                        self.provider
                            .web_url(&h.repo, &h.path, &h.branch, Some(h.line), true)
                            .ok()
                    })
                } else if let Some((owner, repo)) = self.browser.repo_coords() {
                    // File under the cursor yanks the FILE (blob URL);
                    // otherwise the current directory (tree URL).
                    let (path, is_file) = match self.browser.selected_file() {
                        Some((file, _sha)) => (file, true),
                        None => (self.browser.dir_path(), false),
                    };
                    let branch = self.browser.branch().unwrap_or("");
                    self.provider
                        .web_url(&format!("{owner}/{repo}"), &path, branch, None, is_file)
                        .ok()
                } else {
                    self.browser
                        .selected_org()
                        .and_then(|org| self.provider.org_url(&org).ok())
                };
                match url {
                    Some(u) => {
                        self.pending_clipboard = Some(u.clone());
                        self.status = Some(format!("yanked {u}"));
                    }
                    None => self.status = Some("nothing to yank".into()),
                }
            }
            Action::LeaderSearch => {
                // Resume: prefill with the last repo — one Enter
                // re-runs the query back to where the user was.
                self.popup = Some(SearchPopup::with_prefill(self.state.last_repo.as_deref()));
                self.mode = Mode::Browse;
            }
            Action::LeaderFileFind | Action::LeaderGrep => {
                let kind = if action == Action::LeaderFileFind {
                    SearchKind::FileFind
                } else {
                    SearchKind::Grep
                };
                let repo = self
                    .browser
                    .repo_coords()
                    .map(|(owner, name)| format!("{owner}/{name}"));
                let org = self.browser.selected_org();
                let persisted_scope = self
                    .state
                    .search_scope
                    .as_deref()
                    .and_then(crate::components::global_search::Scope::from_stored);
                self.search_view = Some(GlobalSearch::new(
                    kind,
                    repo,
                    org,
                    persisted_scope,
                    self.state.search_extension.clone(),
                ));
                self.mode = Mode::Browse;
            }
            Action::CloseSearchView => {
                self.search_view = None;
                self.mode = Mode::Browse;
            }
            Action::GlobalSearchSubmitted {
                ref kind,
                ref query,
                ref scope,
                ref extension,
            } => {
                // Persist last-used scope/extension (plans/0002 §6.4).
                if let Some(view) = &self.search_view {
                    self.state.search_scope = Some(view.scope().stored().to_string());
                    self.state.search_extension = Some(view.extension_value());
                    self.state.save();
                }
                self.view_gen += 1;
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
                if self.offline {
                    // Tests: inject the mock producer, same Action flow.
                    let hits =
                        crate::components::global_search::mock::hits(*kind, query, extension);
                    let hits = self.finish_hits(hits, *kind, query);
                    if let Some(view) = &mut self.search_view {
                        view.update(&Action::GlobalSearchResults { hits });
                    }
                    self.status = None;
                } else {
                    self.status = Some("searching code…".into());
                    self.spawn_view_search(
                        self.view_gen,
                        *kind,
                        query.clone(),
                        scope.clone(),
                        extension.clone(),
                    );
                }
            }
            Action::GlobalSearchResults { .. } | Action::GlobalSearchFailed { .. } => {
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
                self.status = None;
            }
            Action::OpenSearchHit(hit) => {
                if !hit.sha.is_empty() {
                    // Real hit: materialize the blob like the browser
                    // does (cache-first; the UI is about to suspend).
                    if hit.repo.contains('/') {
                        match crate::editor::prepare(
                            &self.config,
                            self.provider.as_ref(),
                            &hit.repo,
                            &hit.path,
                            &hit.sha,
                        ) {
                            Ok(job) => self.pending_editor = Some(job),
                            Err(message) => {
                                self.status = Some(format!("editor: {message}"));
                            }
                        }
                    }
                } else {
                    // Mock hits: materialize the stand-in body.
                    let slug = self
                        .search_view
                        .as_ref()
                        .map(|v| v.kind().slug())
                        .unwrap_or("hit");
                    match crate::editor::resolve_program(&self.config) {
                        Some(program) => {
                            // Content-address the mock body like a real blob.
                            use sha2::{Digest, Sha256};
                            let sha = format!("{:x}", Sha256::digest(hit.body.as_bytes()));
                            match crate::editor::materialize(
                                "mock",
                                slug,
                                &sha,
                                &hit.path,
                                hit.body.as_bytes(),
                            ) {
                                Ok(file) => {
                                    let mut args =
                                        crate::editor::build_args(&program, &self.config);
                                    args.push(file.to_string_lossy().into_owned());
                                    self.pending_editor =
                                        Some(crate::editor::EditorJob { program, args });
                                }
                                Err(e) => self.status = Some(format!("editor: {e}")),
                            }
                        }
                        None => {
                            self.status =
                                Some("no editor found — set [editor].program or $EDITOR".into());
                        }
                    }
                }
            }
            Action::RunClone { repos, dest } => {
                if repos.is_empty() {
                    self.status = Some("nothing selected to clone".into());
                    self.wizard = None;
                } else {
                    let count = repos.len();
                    self.status = Some(format!("cloning {count} repos…"));
                    self.wizard = None;
                    if !self.offline {
                        self.spawn_clones(repos, dest);
                    }
                }
            }
            Action::ApplySettings(config) => {
                self.settings = None; // close the popup
                let theme_changed = config.theme != self.config.theme;
                let provider_changed = config.provider != self.config.provider;
                self.config = config;
                // Hot reload: rebuild the palette; every component
                // reads Theme per render, so the repaint is automatic.
                if theme_changed {
                    let name = self.config.theme.name.clone();
                    self.theme = Theme::load(&name);
                }
                match self.config.save() {
                    Ok(()) => {
                        self.status = Some(if provider_changed {
                            "settings saved — provider applies after restart".into()
                        } else {
                            "settings saved".into()
                        })
                    }
                    Err(e) => self.status = Some(format!("settings: {e} (applied live)")),
                }
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
            Action::MoveUp
            | Action::MoveDown
            | Action::DrillIn
            | Action::DrillOut
            | Action::PreviewScrollUp
            | Action::PreviewScrollDown => {
                let follow = self.browser.update(&action);
                self.handle_action(follow);
            }
            Action::OpenSelected => {
                match self.browser.selected_kind() {
                    Some(EntryKind::File) => {
                        // Enter on a file → open in the editor (read-only,
                        // PLAN.md §12). The blocking blob fetch is fine:
                        // the UI is about to suspend anyway.
                        if let (Some((path, sha)), Some((owner, repo))) =
                            (self.browser.selected_file(), self.browser.repo_coords())
                        {
                            match crate::editor::prepare(
                                &self.config,
                                self.provider.as_ref(),
                                &format!("{owner}/{repo}"),
                                &path,
                                &sha,
                            ) {
                                Ok(job) => self.pending_editor = Some(job),
                                Err(message) => self.status = Some(format!("editor: {message}")),
                            }
                        }
                    }
                    Some(EntryKind::Dir | EntryKind::Repo | EntryKind::Org) => {
                        let follow = self.browser.update(&Action::DrillIn);
                        self.handle_action(follow);
                    }
                    None => {}
                }
            }
            Action::Noop => {}
        }

        // Incremental filter: re-apply on every SEARCH keystroke.
        if self.mode == Mode::Search {
            self.browser.apply_filter();
        }

        // Any state change can leave a file under the cursor without its
        // blob (navigation, filter commit/clear, tree loads) — drain it
        // uniformly at the end of every route.
        self.maybe_load_blob();
    }

    /// If the selected file's blob isn't loaded, fetch it.
    fn maybe_load_blob(&mut self) {
        if let Some((sha, name)) = self.browser.take_blob_request() {
            self.handle_action(Action::LoadBlob { sha, name });
        }
    }

    /// Hand the prepared editor job to the main loop (which owns the
    /// terminal and performs the suspend/resume dance).
    pub fn take_editor_job(&mut self) -> Option<crate::editor::EditorJob> {
        self.pending_editor.take()
    }

    /// Queued yank text, drained by the main loop once per iteration.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.pending_clipboard.take()
    }

    /// Desired terminal cursor shape, if any text input is focused.
    pub fn cursor_style(&self) -> Option<ratatui::crossterm::cursor::SetCursorStyle> {
        if let Some(cl) = &self.command_line {
            return cl.cursor_style();
        }
        if let Some(settings) = &self.settings {
            return settings.cursor_style();
        }
        if let Some(view) = &self.search_view {
            return view.cursor_style();
        }
        self.popup.as_ref().and_then(|p| p.cursor_style())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        if let Some(view) = &mut self.search_view {
            view.render(frame, rows[0], &self.theme);
            self.modeline.context = view.context();
        } else {
            self.browser.render(frame, rows[0], &self.theme);
            self.modeline.context = self.browser.context();
        }
        self.modeline.status = self.status.clone();
        self.modeline
            .render(frame, rows[1], self.effective_mode(), &self.theme);

        if let Some(popup) = &mut self.popup {
            popup.render(frame, rows[0], &self.theme);
        }
        // v0.3/v0.4 overlays, above the base view.
        if let Some(help) = &mut self.help {
            help.render(frame, rows[0], &self.theme);
        }
        if let Some(settings) = &mut self.settings {
            settings.render(frame, rows[0], &self.theme);
        }
        if let Some(wizard) = &mut self.wizard {
            wizard.render(frame, rows[0], &self.theme);
        }
        // Command strip sits on the modeline's doorstep, last.
        if let Some(command_line) = &mut self.command_line {
            command_line.render(frame, rows[0], &self.theme);
        }
    }
}
/// Worker debug tracing: enabled via GHX_TRACE=/path/log (kept minimal,
/// no logging dependency; remove when the backend stabilizes).
pub fn trace(msg: &str) {
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
