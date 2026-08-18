//! Root: mode stack, action dispatch, component tree (PLAN.md §6).

use crate::action::Action;
use crate::components::browser::Browser;
use crate::components::modeline::Modeline;
use crate::components::pane::EntryKind;
use crate::components::search_popup::SearchPopup;
use crate::components::vim_input::Outcome;
use crate::config::Config;
use crate::keymap;
use crate::mode::Mode;
use crate::theme::Theme;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

pub struct App {
    mode: Mode,
    browser: Browser,
    popup: Option<SearchPopup>,
    modeline: Modeline,
    theme: Theme,
    #[allow(dead_code)]
    config: Config,
    pub should_quit: bool,
    /// Set when stale cells are possible (popup close, resize, editor
    /// resume); the main loop performs a full `terminal.clear()`.
    pub force_redraw: bool,
}

impl App {
    pub fn new() -> Self {
        App {
            mode: Mode::Browse,
            browser: Browser::new(),
            popup: Some(SearchPopup::new()), // launch flow opens on search
            modeline: Modeline::new(),
            theme: Theme::catppuccin_mocha(),
            config: Config::load(),
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
        self.route(action);
    }

    fn dispatch(&mut self, key: KeyEvent) -> Action {
        if let Some(popup) = &mut self.popup {
            return popup.handle_key(key);
        }
        match self.mode {
            Mode::Browse => keymap::browsing(key.code),
            Mode::Search => match self.browser.filter_input.handle_key(key) {
                Outcome::Changed => Action::Noop, // filter applied in route
                Outcome::Submitted => Action::CommitFilter,
                Outcome::Cancelled => Action::ClearFilter,
                Outcome::Noop => Action::Noop,
            },
            Mode::Leader => keymap::leader(key.code),
            _ => Action::Noop,
        }
    }

    fn route(&mut self, action: Action) {
        match action {
            Action::Quit | Action::LeaderQuit => self.should_quit = true,
            Action::ClosePopup => {
                self.popup = None;
                self.mode = Mode::Browse;
                // No full clear: the browser renders full-screen every
                // frame, so the next diff overwrites the popup's cells.
                // A terminal.clear() here would flash (PLAN.md §9).
            }
            Action::RepoSelected { owner, name } => {
                self.browser.set_repo(&owner, &name);
                self.popup = None;
                self.mode = Mode::Browse;
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
                self.browser.update(&action);
            }
            Action::OpenSelected => {
                if matches!(
                    self.browser.selected_kind(),
                    Some(EntryKind::Dir | EntryKind::Repo | EntryKind::Org)
                ) {
                    self.browser.update(&Action::DrillIn);
                }
                // Files: editor integration is milestone 6.
            }
            Action::Noop => {}
        }

        // Incremental filter: re-apply on every SEARCHING keystroke.
        if self.mode == Mode::Search {
            self.browser.apply_filter();
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        self.browser.render(frame, rows[0], &self.theme);
        self.modeline.context = self.browser.context();
        self.modeline
            .render(frame, rows[1], self.effective_mode(), &self.theme);

        if let Some(popup) = &mut self.popup {
            popup.render(frame, rows[0], &self.theme);
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
