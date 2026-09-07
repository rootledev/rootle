//! Root: mode stack, action dispatch, component tree (PLAN.md §6).
//! GitHub calls run on worker threads (blocking reqwest); results return
//! over an mpsc channel as `AppEvent`s, drained once per event-loop tick.
//!
//! `App::with` constructs an **offline** app for tests: no workers are
//! spawned; backend outcomes are injected via `handle_action`.

mod effects;
mod input;
mod presentation;

use crate::action::Action;
use crate::components::Component;
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
use crate::provider;
use crate::request::{CommitGeneration, SearchGeneration, ViewGeneration};
use crate::state::State;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Paragraph;
use rootle_provider::Provider;
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
    last_commits: std::collections::HashMap<(String, String, String), rootle_provider::LogEntry>,
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
    search_gen: SearchGeneration,
    /// Independent from repository search; cross-pipeline compares do not compile.
    view_gen: ViewGeneration,
    commit_generation: CommitGeneration,
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
pub(crate) fn provider_status(error: &rootle_provider::ProviderError) -> String {
    use rootle_provider::ErrorKind;
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
        let (provider, outcome) = provider::build(&config);
        let mut app = Self::build(State::load(), tx, provider, false, config, theme);
        match outcome {
            provider::BuildOutcome::Ready => {}
            // 0022 M1: fallbacks stay visible — sticky, not transient.
            provider::BuildOutcome::Warn(warning) => {
                app.degraded = Some(warning.clone());
                app.status = Some(warning);
            }
            // 0022 M2: the health prompt — retry / browse github /
            // edit config. github carries the session meanwhile.
            provider::BuildOutcome::Health(issue) => {
                app.consent = Some(ConsentPopup::health(issue));
            }
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
        if !app.offline && app.config.update.check && crate::selfupdate::check_allowed() {
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
                degraded: false,
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
            search_gen: SearchGeneration::default(),
            view_gen: ViewGeneration::default(),
            commit_generation: CommitGeneration::default(),
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

    /// Test hooks (0022): simulate a fallback outcome. (Integration
    /// tests link the lib without cfg(test) — plain pub like App::with.)
    pub fn set_degraded_for_test(&mut self, note: String) {
        self.degraded = Some(note.clone());
        self.status = Some(note);
    }

    pub fn clear_status_for_test(&mut self) {
        self.status = None;
    }
}
use rootle_provider::trace;

#[cfg(test)]
mod tests {
    use rootle_provider::{ErrorKind, ProviderError};
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
