//! Input for App.

use super::{Action, App, Component, KeyEvent, Mode, Outcome, keymap};

impl App {
    pub(super) fn effective_mode(&self) -> Mode {
        if self.consent.is_some() {
            return Mode::Browse;
        }
        if let Some(wizard) = &self.wizard {
            return wizard.effective_mode();
        }
        if let Some(settings) = &self.settings {
            return settings.effective_mode();
        }
        if let Some(help) = &self.help {
            return help.effective_mode();
        }
        if let Some(refs) = &self.refs_popup {
            return refs.effective_mode();
        }
        if self.command_line.is_some() {
            return Mode::Insert;
        }
        if self.mode == Mode::History && self.browser.history_filtering() {
            return Mode::Search;
        }
        if self.mode == Mode::Commit
            && self
                .browser
                .commit_ref()
                .is_some_and(|view| view.filtering())
        {
            return Mode::Search;
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

    pub(super) fn dispatch(&mut self, key: KeyEvent) -> Action {
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
                    Action::HistoryFilterKey(key)
                } else {
                    keymap::history(key.code)
                }
            }
            Mode::Preview => self.browser.preview_key(key),
            Mode::Commit => self
                .browser
                .commit()
                .map(|view| view.handle_key(key))
                .unwrap_or(Action::Noop),
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
}
