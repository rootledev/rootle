//! Presentation for App.

use super::{
    App, Constraint, Direction, Frame, Layout, Mode, Paragraph, Rect, SettingsPopup, Theme,
};

impl App {
    /// The theme everything renders with: the settings popup's live
    /// preview while it's browsing palettes, the committed theme
    /// otherwise.
    pub(super) fn effective_theme(&self) -> Theme {
        self.settings
            .as_ref()
            .and_then(SettingsPopup::preview_theme)
            .unwrap_or(self.theme)
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
        if let Some(popup) = &self.popup {
            return popup.cursor_style();
        }
        (self.mode == Mode::Commit
            && self
                .browser
                .commit_ref()
                .is_some_and(|view| view.searching()))
        .then_some(ratatui::crossterm::cursor::SetCursorStyle::SteadyBar)
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
            || self.consent.is_some()
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
            let zoomed = matches!(self.mode, Mode::Preview | Mode::History | Mode::Commit)
                || (self.mode == Mode::Find && self.find_return == Some(Mode::Preview));
            self.browser.preview.focused = zoomed;
            self.browser.render(frame, rows[0], &theme, zoomed);
            self.modeline.context = self.browser.context();
        }
        if strip {
            frame.render_widget(
                Paragraph::new(crate::components::modeline::hint_strip_line(
                    if mode == Mode::History && self.browser.repository_history_active() {
                        crate::keymap::repository_history_hints()
                    } else if mode == Mode::Commit {
                        self.browser
                            .commit_ref()
                            .map(|view| view.hints())
                            .unwrap_or_else(|| crate::keymap::hints(mode))
                    } else {
                        crate::keymap::hints(mode)
                    },
                    rows[1].width as usize,
                    &theme,
                )),
                rows[1],
            );
        }
        let mut status = self.status.clone().or_else(|| self.degraded.clone());
        // 0030: a failed requested trace rides along as a sticky
        // suffix — it must never displace the primary status.
        if let Some(note) = &self.trace_failure {
            status = Some(match status {
                Some(current) => format!("{current} · {note}"),
                None => note.clone(),
            });
        }
        self.modeline.status = status;
        let modeline_row = rows[rows.len() - 1];
        self.modeline.update_tag = self.update_tag.clone();
        // 0022 M3: the forge chip tints warning while degraded.
        self.modeline.degraded = self.degraded.is_some();
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

    /// Headless state dump (plans/0023 M1): one JSON object per
    /// `state` step — what a scripted reviewer needs to assert on
    /// without parsing the frame.
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": self.effective_mode().chip(),
            "context": self.browser.context(),
            "ref": self.browser.current_ref(),
            "provider": self.provider.name(),
            "popup": self.popup.is_some(),
            "search_view": self.search_view.is_some(),
            "help": self.help.is_some(),
            "command_line": self.command_line.is_some(),
            "settings": self.settings.is_some(),
            "wizard": self.wizard.is_some(),
            "refs_popup": self.refs_popup.is_some(),
            "consent": self.consent.is_some(),
            "status": self.status,
            "degraded": self.degraded,
            "update_tag": self.update_tag,
            "should_quit": self.should_quit,
            "surface": self.commit_surface(),
        })
    }

    /// The commit viewer's surface state, for headless scripts
    /// (plans/0028 M3): null when no viewer, else the open surface
    /// kind + position.
    pub(super) fn commit_surface(&self) -> serde_json::Value {
        let Some(view) = self.browser.commit_ref() else {
            return serde_json::Value::Null;
        };
        serde_json::json!({
            "sha": view.sha_short(),
            "repository": view.request().repository.as_str(),
            "request": view.request().generation.to_string(),
            "delta": view.open_file().map(|index| index.get()),
            "files": view.file_count(),
        })
    }

    pub fn active_commit_request(&self) -> Option<&crate::request::CommitRequest> {
        self.browser.commit_ref().map(|view| view.request())
    }

    pub fn active_history_request(&self) -> Option<&crate::request::HistoryRequest> {
        self.browser.history_request()
    }
}
