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
use crate::theme::{BorderShape, Theme};
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
    /// The syntax roles the highlighter + blob cache are styled with
    /// (restyle trigger on theme switch).
    highlight_syntax: crate::theme::Syntax,
    /// Generation counter on search submissions; stale results dropped.
    search_gen: u64,
    /// Same for the global search view (find/grep workers).
    view_gen: u64,
    /// sha of the lazy hit-context fetch in flight (plans/0006 §1) —
    /// dedupes repeat selections and names the cancel target.
    pending_context_sha: Option<String>,
    /// Cursor-rest debounce generation (plans/0008 §3): bumped on
    /// every context request; a timer thread fires only if its
    /// generation is still current when the cursor rests.
    context_debounce_gen: std::sync::Arc<std::sync::atomic::AtomicU64>,
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

/// Render a provider error for the status line (plans/0008 §2):
/// auth gets a recovery hint, throttling gets its advertised backoff,
/// everything else is yesterday's plain message.
pub(crate) fn provider_status(error: &crate::provider::ProviderError) -> String {
    use crate::provider::ErrorKind;
    match error.kind {
        ErrorKind::Auth => format!(
            "auth failed: {} — refresh provider credentials",
            error.message
        ),
        ErrorKind::RateLimited => match error.retry_after {
            Some(d) => format!("provider throttled — retry in {}s", d.as_secs()),
            None => format!("provider throttled: {}", error.message),
        },
        _ => error.message.clone(),
    }
}

/// Forge chip text for the modeline: `[provider] name` when set, else
/// the provider's self-reported name (`stdio:name` → `name`).
fn forge_name(config: &Config, provider: &dyn Provider) -> String {
    config
        .provider
        .name
        .clone()
        .unwrap_or_else(|| provider.name().trim_start_matches("stdio:").to_owned())
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
        let forge = forge_name(&config, provider.as_ref());
        App {
            mode: Mode::Browse,
            browser: Browser::new(&state.recent_orgs, &provider.default_orgs()),
            popup, // opens on launch only for a fresh state
            search_view: None,
            help: None,
            command_line: None,
            settings: None,
            wizard: None,
            modeline: Modeline {
                forge,
                context: String::new(),
                status: None,
            },
            theme,
            config,
            state,
            tx,
            provider,
            highlighter: Highlighter::new(&theme),
            // The syntax roles the highlighter + blob cache are styled
            // with; compared against the effective theme to trigger
            // restyle (settings live preview / commit).
            highlight_syntax: theme.syntax,
            search_gen: 0,
            view_gen: 0,
            pending_context_sha: None,
            context_debounce_gen: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
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
            AppEvent::SearchFailed { gen_id, error } => {
                if gen_id != self.search_gen {
                    return;
                }
                self.status = None;
                if let Some(popup) = &mut self.popup {
                    popup.update(&Action::SearchFailed { error });
                }
            }
            AppEvent::OrgReposLoaded { org, repos } => {
                self.status = None;
                self.browser.org_repos_loaded(&org, repos);
            }
            AppEvent::OrgReposFailed { org, error } => {
                self.status = Some(format!("{org}: {}", provider_status(&error)));
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
            AppEvent::BlobFailed { sha, error } => {
                self.handle_action(Action::BlobFailed { sha, error });
            }
            AppEvent::TreeFailed { owner, name, error } => {
                self.handle_action(Action::TreeFailed { owner, name, error });
            }
            AppEvent::GlobalSearchDelta { gen_id, hits } => {
                if gen_id != self.view_gen {
                    return; // stale batch — a newer submission owns the view
                }
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
                    view.update(&Action::GlobalSearchDelta { hits });
                }
                // Live count while the stream runs.
                if let Some(view) = &self.search_view {
                    self.status = Some(format!(
                        "searching {}… {} hits",
                        self.modeline.forge,
                        view.hit_count()
                    ));
                }
            }
            AppEvent::GlobalSearchResults {
                gen_id,
                hits,
                clipped,
            } => {
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
                    view.update(&Action::GlobalSearchResults { hits, clipped });
                }
                // Bare selected hit (beyond the eager preview cap): ask
                // for its context lazily (plans/0006 §1).
                let request = self
                    .search_view
                    .as_ref()
                    .and_then(|view| view.context_request());
                if let Some(action) = request {
                    self.handle_action(action);
                }
            }
            AppEvent::GlobalSearchFailed { gen_id, error } => {
                if gen_id != self.view_gen {
                    return;
                }
                self.status = None;
                if let Some(view) = &mut self.search_view {
                    view.update(&Action::GlobalSearchFailed { error });
                }
            }
            AppEvent::HitContextDebounceFired {
                timer_gen,
                hit,
                query,
            } => {
                self.handle_action(Action::HitContextDebounceFired {
                    timer_gen,
                    hit,
                    query,
                });
            }
            AppEvent::HitContextMissing { gen_id, sha } => {
                if gen_id != self.view_gen {
                    return; // view moved on
                }
                self.handle_action(Action::HitContextMissing { sha });
            }
            AppEvent::HitContextFailed { gen_id, sha, error } => {
                if gen_id != self.view_gen {
                    return;
                }
                self.handle_action(Action::HitContextFailed { sha, error });
            }
            AppEvent::HitContextLoaded {
                gen_id,
                repo,
                path,
                sha,
                line,
                preview,
                match_count,
                query,
            } => {
                if gen_id != self.view_gen {
                    return; // view moved on
                }
                if self.pending_context_sha.as_deref() == Some(sha.as_str()) {
                    self.pending_context_sha = None;
                }
                let Some(view) = &self.search_view else {
                    return;
                };
                let kind = view.kind();
                let mut hits = vec![crate::components::global_search::SearchHit::plain(
                    &repo,
                    &path,
                    line,
                    preview,
                    match_count,
                    String::new(),
                )];
                hits = self.finish_hits(hits, kind, &query);
                let styled = hits.pop().expect("one hit");
                let action = Action::HitContextLoaded {
                    repo,
                    path,
                    sha,
                    line,
                    preview: styled.preview,
                    match_count,
                };
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
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
            Mode::Find => match self.browser.find_input.handle_key(key) {
                Outcome::Changed => Action::UpdateFind,
                Outcome::Submitted => Action::CommitFind,
                Outcome::Cancelled => Action::CancelFind,
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
                self.provider.advise_cancel(); // superseded in-flight work
                self.status = Some(format!("searching {}…", self.modeline.forge));
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
            Action::OrgReposFailed { org, error } => {
                self.status = Some(format!("{org}: {}", provider_status(&error)));
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
            Action::TreeFailed { owner, name, error } => {
                self.status = Some(format!("{owner}/{name}: {}", provider_status(&error)));
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
                let lang = self.highlighter.language(&name);
                self.browser.blob_loaded(&sha, &name, &lang, text, lines);
            }
            Action::BlobFailed { sha, error } => {
                self.status = Some(provider_status(&error));
                self.browser.blob_failed(&sha, &error.message);
            }
            Action::HitContextFailed { sha: _, error } => {
                // Auth/throttle surface; anything else stays quiet —
                // the bare path remains and revisit retries (§2).
                use crate::provider::ErrorKind;
                if matches!(error.kind, ErrorKind::Auth | ErrorKind::RateLimited) {
                    self.status = Some(provider_status(&error));
                }
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
                    // File yank anchors to the preview line cursor
                    // (plans/0006 §5); dirs/orgs stay line-less.
                    let line = if is_file {
                        self.browser.preview_line()
                    } else {
                        None
                    };
                    self.provider
                        .web_url(&format!("{owner}/{repo}"), &path, branch, line, is_file)
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
                self.provider.advise_cancel(); // superseded in-flight work
                self.pending_context_sha = None;
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
                if self.offline {
                    // Tests: inject the mock producer, same Action flow.
                    let hits =
                        crate::components::global_search::mock::hits(*kind, query, extension);
                    let hits = self.finish_hits(hits, *kind, query);
                    if let Some(view) = &mut self.search_view {
                        view.update(&Action::GlobalSearchResults {
                            hits,
                            clipped: false,
                        });
                    }
                    self.status = None;
                    // Bare selected hit (beyond the eager preview cap):
                    // ask for its context lazily (plans/0006 §1).
                    let request = self
                        .search_view
                        .as_ref()
                        .and_then(|view| view.context_request());
                    if let Some(action) = request {
                        self.handle_action(action);
                    }
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
            Action::GlobalSearchResults { .. }
            | Action::GlobalSearchDelta { .. }
            | Action::GlobalSearchFailed { .. } => {
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
                self.status = None;
            }
            Action::LoadHitContext { hit, query } => {
                // Dedupe repeat selections of an in-flight fetch.
                if self.pending_context_sha.as_deref() == Some(hit.sha.as_str()) {
                    return;
                }
                if self.offline {
                    return; // tests inject context via Action directly
                }
                // Cursor-rest debounce (plans/0008 §3): 200ms rearmed
                // per selection change. Holding j through N hits costs
                // one provider call — the resting one — instead of N
                // requests plus N-1 advisory cancels.
                let timer_gen = self
                    .context_debounce_gen
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                let shared = self.context_debounce_gen.clone();
                let tx = self.tx.clone();
                let hit = *hit;
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    if shared.load(std::sync::atomic::Ordering::SeqCst) == timer_gen {
                        let _ = tx.send(AppEvent::HitContextDebounceFired {
                            timer_gen,
                            hit,
                            query,
                        });
                    }
                });
            }
            Action::HitContextDebounceFired {
                timer_gen,
                hit,
                query,
            } => {
                // The timer thread already generation-checked; re-check
                // here — another request may have landed while the
                // event queued.
                if timer_gen
                    != self
                        .context_debounce_gen
                        .load(std::sync::atomic::Ordering::SeqCst)
                {
                    return;
                }
                if self.pending_context_sha.as_deref() == Some(hit.sha.as_str()) {
                    return;
                }
                // A different pending fetch is superseded — tell the
                // provider it can stop (v1.1).
                if self.pending_context_sha.is_some() {
                    self.provider.advise_cancel();
                }
                self.pending_context_sha = Some(hit.sha.clone());
                let gen_id = self.view_gen;
                self.spawn_hit_context(gen_id, hit, query);
            }
            Action::HitContextLoaded { .. } | Action::HitContextMissing { .. } => {
                if let Some(view) = &mut self.search_view {
                    view.update(&action);
                }
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
                            // digest 0.11 dropped `LowerHex` on its output —
                            // render the hex by hand.
                            use std::fmt::Write as _;
                            let digest = Sha256::digest(hit.body.as_bytes());
                            let mut sha = String::with_capacity(digest.len() * 2);
                            for byte in digest {
                                let _ = write!(sha, "{byte:02x}");
                            }
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
                let ui_changed = config.ui != self.config.ui;
                self.config = config;
                // Hot reload: rebuild the palette (and border shape);
                // every component reads Theme per render, so the
                // repaint is automatic.
                if theme_changed || ui_changed {
                    let name = self.config.theme.name.clone();
                    let border = BorderShape::parse(&self.config.ui.border).unwrap_or_default();
                    self.theme = Theme::load(&name).with_border(border);
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
                // BROWSE Esc precedence (plans/0007 §3): a committed
                // find clears first (:nohlsearch), then the list filter.
                // In SEARCH mode Esc keeps its cancel-the-session role.
                if self.mode == Mode::Browse && self.browser.preview.find_active() {
                    self.browser.preview.clear_find();
                } else {
                    self.browser.clear_filter();
                }
                self.mode = Mode::Browse;
            }
            Action::LeaderFindInFile => {
                self.mode = Mode::Browse; // leader layer down either way
                if self.browser.preview.findable() {
                    self.browser.find_input.clear();
                    self.browser.find_input.submode = crate::components::vim_input::SubMode::Insert;
                    self.browser.preview.begin_find();
                    self.mode = Mode::Find;
                } else {
                    self.status = Some("find: preview is not a text file".into());
                }
            }
            Action::UpdateFind => {
                let query = self.browser.find_input.value();
                self.browser.preview.update_find(query);
            }
            Action::CommitFind => self.mode = Mode::Browse,
            Action::CancelFind => {
                self.browser.preview.cancel_find();
                self.browser.find_input.clear();
                self.mode = Mode::Browse;
            }
            Action::FindNext => {
                self.browser.preview.find_step(1);
            }
            Action::FindPrev => {
                self.browser.preview.find_step(-1);
            }
            Action::MoveUp
            | Action::MoveDown
            | Action::DrillIn
            | Action::DrillOut
            | Action::PreviewLineUp
            | Action::PreviewLineDown => {
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

        // Theme switches (settings live preview, then commit) restyle
        // the code the same frame the chrome recolors.
        self.sync_highlight_theme();

        // Any state change can leave a file under the cursor without its
        // blob (navigation, filter commit/clear, tree loads) — drain it
        // uniformly at the end of every route.
        self.maybe_load_blob();

        // Provider notices (a stdio child's successful restart) ride
        // the status line once (plans/0008 §5).
        if let Some(note) = self.provider.take_notice() {
            self.status = Some(note);
        }
    }

    /// The theme everything renders with: the settings popup's live
    /// preview while it's browsing palettes, the committed theme
    /// otherwise.
    fn effective_theme(&self) -> Theme {
        self.settings
            .as_ref()
            .and_then(SettingsPopup::preview_theme)
            .unwrap_or(self.theme)
    }

    /// Re-highlight cached blobs when the effective theme's syntax
    /// roles change. Cheap no-op per keystroke otherwise (SyntaxSet is
    /// loaded once; only the color table rebuilds).
    fn sync_highlight_theme(&mut self) {
        let theme = self.effective_theme();
        if theme.syntax == self.highlight_syntax {
            return;
        }
        self.highlighter.set_theme(&theme);
        self.browser.restyle_blobs(&self.highlighter);
        self.highlight_syntax = theme.syntax;
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
        let theme = self.effective_theme();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        if let Some(view) = &mut self.search_view {
            view.render(frame, rows[0], &theme);
            self.modeline.context = view.context();
        } else {
            self.browser.render(frame, rows[0], &theme);
            self.modeline.context = self.browser.context();
        }
        self.modeline.status = self.status.clone();
        self.modeline
            .render(frame, rows[1], self.effective_mode(), &theme);

        if let Some(popup) = &mut self.popup {
            popup.render(frame, rows[0], &theme);
        }
        // v0.3/v0.4 overlays, above the base view.
        if let Some(help) = &mut self.help {
            help.render(frame, rows[0], &theme);
        }
        if let Some(settings) = &mut self.settings {
            settings.render(frame, rows[0], &theme);
        }
        if let Some(wizard) = &mut self.wizard {
            wizard.render(frame, rows[0], &theme);
        }
        // Command strip sits on the modeline's doorstep, last.
        if let Some(command_line) = &mut self.command_line {
            command_line.render(frame, rows[0], &theme);
        }
    }
}
/// Worker debug tracing: enabled via ROOTLE_TRACE=/path/log (kept minimal,
/// no logging dependency; remove when the backend stabilizes).
pub fn trace(msg: &str) {
    if let Ok(path) = std::env::var("ROOTLE_TRACE") {
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

#[cfg(test)]
mod tests {
    use crate::provider::{ErrorKind, ProviderError};
    use std::time::Duration;

    #[test]
    fn provider_status_renders_per_kind() {
        let auth = ProviderError::new(ErrorKind::Auth, "bad credentials");
        let rendered = super::provider_status(&auth);
        assert!(rendered.contains("bad credentials"));
        assert!(rendered.contains("refresh provider credentials"));

        let throttled = ProviderError::new(ErrorKind::RateLimited, "slow down")
            .with_retry_after(Duration::from_secs(37));
        assert_eq!(
            super::provider_status(&throttled),
            "provider throttled — retry in 37s"
        );

        let plain = ProviderError::other("something broke");
        assert_eq!(super::provider_status(&plain), "something broke");
    }
}
