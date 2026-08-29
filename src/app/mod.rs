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
use crate::components::consent_popup::ConsentPopup;
use crate::components::global_search::GlobalSearch;
use crate::components::keybinds_popup::KeybindsPopup;
use crate::components::modeline::Modeline;
use crate::components::refs_popup::RefsPopup;
use crate::components::search_popup::SearchPopup;
use crate::components::settings_popup::SettingsPopup;
use crate::components::vim_input::Outcome;
use crate::config::Config;
use crate::event::AppTx;
use crate::highlight::Highlighter;
use crate::keymap;
use crate::mode::Mode;
use crate::provider::{self, Provider};
use crate::state::State;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Paragraph;
use std::sync::Arc;

mod actions;
mod events;
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
    /// Revision switcher overlay (plans/0016 M1a mock) and the
    /// revision committed when it opened — Esc reverts the crumb.
    refs_popup: Option<RefsPopup>,
    refs_baseline: Option<String>,
    /// 0019 M2: declared-but-missing provider — the consent popup
    /// owns startup until answered (y installs, n degrades honestly).
    consent: Option<ConsentPopup>,
    /// 0019 M2: sticky degradation notice (declared provider
    /// unavailable — the honest-channel surface). Transient statuses
    /// overlay it; nothing clears it for the session.
    degraded: Option<String>,
    /// 0019 polish: last-commit memo for the preview band, keyed
    /// (repo, path, ref). The band is ambient — re-selects never
    /// refetch; the first preview of a file spawns one `log(limit=1)`.
    last_commits: std::collections::HashMap<(String, String, String), crate::provider::LogEntry>,
    /// closes (history from blame, find from preview).
    history_return: Option<Mode>,
    find_return: Option<Mode>,
    /// A newer release tag when the startup check found one (0017 M3)
    /// — the modeline's `↑ vX.Y.Z` chip.
    update_tag: Option<String>,
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
    /// Fetch the open history lens' commits (v1.5): the previewed
    /// file's log at the browsed revision.
    fn open_history_fetch(&mut self) {
        let target = self
            .browser
            .repo_coords()
            .zip(self.browser.history_path().map(str::to_string));
        if let Some(((owner, name), path)) = target {
            let ref_ = self.browser.current_ref().map(str::to_string);
            self.spawn_log(format!("{owner}/{name}"), path, ref_);
        }
    }

    pub fn new(tx: AppTx, config: Config, theme: Theme) -> Self {
        let (provider, outcome) = provider::build(&config);
        let mut app = Self::build(State::load(), tx, provider, false, config, theme);
        match outcome {
            provider::BuildOutcome::Ready => {}
            provider::BuildOutcome::Warn(warning) => app.status = Some(warning),
            // 0019 M2: a declared provider is missing — ask, never
            // silently download-and-run. github carries the session
            // while the popup is up.
            provider::BuildOutcome::Missing(decl) => {
                app.consent = Some(ConsentPopup::new(decl));
            }
        }
        // 0017 M3 / 0018 M2: the 24h-cached update notice — never
        // offline, never blocking, silent on failure; CI, dumb
        // terminals, and piped stdout never check at all.
        if !app.offline && app.config.update.check && crate::update::check_allowed() {
            app.spawn_update_check();
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
        let forge = forge_name(&config, provider.as_ref());
        let icon = config.provider.icon.clone().or_else(|| provider.icon());
        let popup = if state.recent_repos.is_empty()
            && state.recent_orgs.is_empty()
            && state.last_repo.is_none()
        {
            let mut p = SearchPopup::with_prefill(state.last_repo.as_deref());
            p.forge = forge.clone();
            Some(p)
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
            refs_popup: None,
            refs_baseline: None,
            // 0019 M2: declared-but-missing provider — the consent
            consent: None,
            degraded: None,
            last_commits: std::collections::HashMap::new(),
            history_return: None,
            find_return: None,
            update_tag: None,
            modeline: Modeline {
                forge,
                icon,
                context: String::new(),
                status: None,
                update_tag: None,
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

    fn dispatch(&mut self, key: KeyEvent) -> Action {
        // Overlays capture keys, topmost first — the consent popup is
        // the very top: a pending trust decision outranks everything.
        if let Some(consent) = &mut self.consent {
            return consent.handle_key(key);
        }
        if let Some(wizard) = &mut self.wizard {
            return wizard.handle_key(key);
        }
        if let Some(settings) = &mut self.settings {
            return settings.handle_key(key);
        }
        if let Some(help) = &mut self.help {
            return help.handle_key(key);
        }
        if let Some(refs) = &mut self.refs_popup {
            return refs.handle_key(key);
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
            Mode::History => {
                // An active `/` session owns the keys until commit.
                if self.browser.history_filtering() {
                    self.browser.history_filter_key(key);
                    Action::Noop
                } else {
                    keymap::history(key.code)
                }
            }
            Mode::Preview => self.browser.preview_key(key),
            _ => Action::Noop,
        }
    }

    /// Dispatch: domain files own the arms (plans/0021 M1) —
    /// `try_*` returns the action back when it isn't theirs. The
    /// shared tail (filter re-apply, theme sync, blob drain, provider
    /// notices) applies to every routed action.
    pub fn handle_action(&mut self, action: Action) {
        let left = self
            .try_browse(action)
            .and_then(|a| self.try_search(a))
            .and_then(|a| self.try_lenses(a))
            .and_then(|a| self.try_lifecycle(a));
        debug_assert!(left.is_none(), "unrouted action: {left:?}");
        let _ = left;
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

        // Provider notices ride the status line once (plans/0008 §5).
        // 0022 M1: a restart-failure streak goes sticky; successes
        // stay transient.
        if let Some(note) = self.provider.take_notice() {
            if note.contains("keeps failing to restart") {
                self.degraded = Some(note.clone());
            }
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

    /// 0019 polish: the band's last-commit context for the file under
    /// preview — memo hit dresses immediately, a miss spawns the
    /// one-shot fetch (ambient; errors stay silent).
    fn band_apply_or_fetch(&mut self) {
        if !self.provider.capabilities().log || self.browser.at_commit_view() {
            return;
        }
        let Some((owner, name)) = self.browser.repo_coords() else {
            return;
        };
        let Some((path, _)) = self.browser.selected_file() else {
            return;
        };
        let ref_ = self
            .browser
            .current_ref()
            .map(str::to_string)
            .unwrap_or_default();
        let key = (format!("{owner}/{name}"), path.clone(), ref_.clone());
        match self.last_commits.get(&key) {
            Some(entry) => {
                self.browser.preview.set_band(
                    Some(path),
                    Some(crate::components::preview::BandContext {
                        sha: entry.sha.clone(),
                        subject: entry.subject.clone(),
                        author: entry.author.clone(),
                        date: entry.date.clone(),
                    }),
                );
            }
            None => {
                self.spawn_last_commit(format!("{owner}/{name}"), path, Some(ref_));
            }
        }
    }

    /// 0018 M3: the quit-time restart trace — only when this session
    /// knew about an update (the `↑` chip), compare the on-disk
    /// binary once. The caller prints it after terminal restore.
    pub fn update_exit_note(&self) -> Option<String> {
        self.update_tag.as_ref()?;
        crate::update::disk_newer_note()
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
        let mode = self.effective_mode();
        // State vs keys, one hint surface per context: a view or
        // overlay that draws its own border hint row (search view,
        // popups, wizard) wins; the glued strip serves what has no
        // border — the leader layer (always) and the browser's
        // transient modes; the modeline is state-only either way.
        let overlay_up = self.popup.is_some()
            || self.wizard.is_some()
            || self.settings.is_some()
            || self.help.is_some()
            || self.command_line.is_some()
            || self.refs_popup.is_some()
            || self.search_view.is_some();
        let strip = mode == Mode::Leader || (!overlay_up && mode != Mode::Browse);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(if strip {
                vec![
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ]
            } else {
                vec![Constraint::Min(1), Constraint::Length(1)]
            })
            .split(area);

        if let Some(view) = &mut self.search_view {
            view.render(frame, rows[0], &theme);
            self.modeline.context = view.context();
        } else {
            // The preview submode (␣ p) and its lenses zoom the pane to
            // the full row; FIND raised from it keeps the zoom.
            let zoomed = matches!(self.mode, Mode::Preview | Mode::History)
                || (self.mode == Mode::Find && self.find_return == Some(Mode::Preview));
            self.browser.preview.focused = zoomed;
            self.browser.render(frame, rows[0], &theme, zoomed);
            self.modeline.context = self.browser.context();
        }
        if strip {
            frame.render_widget(
                Paragraph::new(crate::components::modeline::hint_strip_line(
                    mode,
                    rows[1].width as usize,
                    &theme,
                )),
                rows[1],
            );
        }
        self.modeline.status = self.status.clone().or_else(|| self.degraded.clone());
        let modeline_row = rows[rows.len() - 1];
        self.modeline.update_tag = self.update_tag.clone();
        self.modeline.render(frame, modeline_row, mode, &theme);

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
        if let Some(refs) = &mut self.refs_popup {
            refs.render(frame, rows[0], &theme);
        }
        // 0019 M2: the consent popup is the topmost surface — a
        // pending trust decision renders above everything.
        if let Some(consent) = &mut self.consent {
            consent.render(frame, rows[0], &theme);
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
