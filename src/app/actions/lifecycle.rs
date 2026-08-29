//! Action dispatch — app lifecycle: quit, popups, command line, settings, the declared-provider consent answers, and the clone run
//! (moved from app/mod.rs, plans/0021 M1 — a pure move, zero behavior
//! change).

use super::super::trace;
use super::super::{App, provider, provider_status};
use crate::action::Action;
use crate::components::clone_wizard::CloneWizard;
use crate::components::command_line::CommandLine;
use crate::components::consent_popup::ConsentPopup;
use crate::components::keybinds_popup::KeybindsPopup;
use crate::components::settings_popup::SettingsPopup;
use crate::mode::Mode;
use crate::theme::BorderShape;
use crate::theme::Theme;

impl App {
    /// This domain's arms: `Some(action)` back when not ours, so the
    /// next domain tries it; arm bodies are moved verbatim.
    pub(crate) fn try_lifecycle(&mut self, action: Action) -> Option<Action> {
        let _consumed: bool = match action {
            Action::Quit | Action::LeaderQuit => {
                self.state.last_path = Some(self.browser.dir_path());
                self.state.save();
                self.should_quit = true;
                true
            }
            Action::ClosePopup => {
                trace("ClosePopup");
                // Topmost overlay first; the search popup last.
                if self.wizard.take().is_some()
                    || self.settings.take().is_some()
                    || self.help.take().is_some()
                    || self.command_line.take().is_some()
                {
                    return None;
                }
                if self.refs_popup.take().is_some() {
                    // Revisions switcher cancelled: the live preview
                    // reverts to the committed revision.
                    let baseline = self.refs_baseline.clone();
                    self.browser.set_current_ref(baseline);
                    return None;
                }
                self.popup = None;
                self.mode = Mode::Browse;
                true
            }
            Action::Leader => {
                self.mode = Mode::Leader;
                true
            }
            Action::KeybindsPopup => {
                self.help = Some(KeybindsPopup::new());
                true
            }
            Action::CommandLine => {
                self.command_line = Some(CommandLine::new());
                true
            }
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
                            // Direct selections carry no listing
                            // metadata (v1.4): bare names.
                            let repos = repos
                                .into_iter()
                                .map(crate::provider::RepoInfo::bare)
                                .collect();
                            self.wizard = Some(CloneWizard::new(repos, cwd));
                        } else {
                            self.status = Some(format!("expanding {} org(s)…", orgs.len()));
                            if !self.offline {
                                self.spawn_expand_clone(repos, orgs);
                            }
                        }
                    }
                    other => {
                        // `:42` jumps to line 42 in a file view — the
                        // zoomed preview submode or a search hit's
                        // expanded file pane (plans/0016 M1).
                        match other.parse::<u32>() {
                            Ok(line) if line > 0 => {
                                let jumped = if let Some(view) = &mut self.search_view {
                                    view.expanded_goto_line(line)
                                } else if self.browser.preview.text_line_count() > 0 {
                                    self.browser.preview.set_cursor_line(line);
                                    true
                                } else {
                                    false
                                };
                                if !jumped {
                                    self.status = Some(format!(":{other}: not in a file view"));
                                }
                            }
                            _ => self.status = Some(format!("unknown command: {other}")),
                        }
                    }
                }
                true
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
                true
            }
            Action::ApplySettings(config) => {
                self.settings = None; // close the popup
                let theme_changed = config.theme != self.config.theme;
                let provider_changed = config.provider != self.config.provider;
                let ui_changed = config.ui != self.config.ui;
                self.config = config;
                // Hot reload: rebuild the palette (and chrome prefs);
                // every component reads Theme per render, so the
                // repaint is automatic.
                if theme_changed || ui_changed {
                    let name = self.config.theme.name.clone();
                    let border = BorderShape::parse(&self.config.ui.border).unwrap_or_default();
                    self.theme = Theme::load(&name)
                        .with_border(border)
                        .with_nerd_font(self.config.ui.nerd_font)
                        .with_separator(&self.config.ui.separator);
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
                true
            }
            Action::DeclarationDecline => {
                if let Some(consent) = self.consent.take() {
                    // 0022: the health popup names its issue, the install
                    // popup its declaration — both decline to github.
                    let name = match consent.health_issue() {
                        Some(issue) => issue.name.clone(),
                        None => consent.declaration().name.clone(),
                    };
                    // Honest degraded mode: the fallback is named, the
                    // declaration stays in the config, the retry is a
                    // command away — and the notice is sticky.
                    let note = format!(
                        "{name} not installed — browsing github (retry: rootle provider install {name})"
                    );
                    self.degraded = Some(note.clone());
                    self.status = Some(note);
                }
                true
            }
            Action::DeclarationState(state) => {
                if let Some(consent) = &mut self.consent {
                    consent.set_state(state);
                }
                true
            }
            Action::Noop => true,
            // 0019 M2: the consent popup's answers.
            // 0021 M3 hygiene: yank from the repo search popup — the
            // selected entry's URL (org page or repo tree).
            Action::SearchYank => {
                if let Some(popup) = &self.popup
                    && let Some(entry) = popup.selected_entry()
                {
                    let url = match entry.kind {
                        crate::components::pane::EntryKind::Org => {
                            self.provider.org_url(&entry.name)
                        }
                        _ => self
                            .provider
                            .web_url(&entry.name, "", "", None, None, false),
                    };
                    match url {
                        Ok(u) => {
                            self.pending_clipboard = Some(u.clone());
                            self.status = Some(format!("yanked {u}"));
                        }
                        Err(e) => self.status = Some(provider_status(&e)),
                    }
                }
                true
            }
            // 0022 M2 health prompt: retry the spawn once — rebuild
            // the provider; swap it in on success, show the error on
            // failure (the popup stays up).
            Action::DeclarationRetry => {
                let (provider, outcome) = provider::build(&self.config);
                match outcome {
                    provider::BuildOutcome::Ready | provider::BuildOutcome::Warn(_) => {
                        self.provider = provider;
                        self.consent = None;
                        self.degraded = None;
                        self.status = Some("provider back".into());
                    }
                    provider::BuildOutcome::Health(issue) => {
                        self.provider = provider;
                        if let Some(popup) = &mut self.consent {
                            popup.set_state(crate::action::DeclarationState::Failed(issue.error));
                        }
                    }
                    provider::BuildOutcome::Missing(decl) => {
                        self.provider = provider;
                        self.consent = Some(ConsentPopup::new(decl));
                    }
                }
                true
            }
            // 0022 M2 health prompt: open the config in the editor —
            // main suspends the terminal for it (the same editor path
            // as file opens).
            Action::DeclarationEditConfig => {
                self.consent = None;
                match (
                    crate::editor::resolve_program(&self.config),
                    crate::config::Config::path(),
                ) {
                    (Some(program), Some(path)) => {
                        let mut args = crate::editor::build_args(&program, &self.config);
                        args.push(path.to_string_lossy().into_owned());
                        self.pending_editor = Some(crate::editor::EditorJob { program, args });
                    }
                    _ => {
                        self.status =
                            Some("no editor found — set [editor].program or $EDITOR".into())
                    }
                }
                true
            }
            Action::DeclarationAccept => {
                if let Some(consent) = &mut self.consent {
                    let decl = consent.declaration().clone();
                    consent.set_state(crate::action::DeclarationState::Installing);
                    self.status = Some(format!("installing {}…", decl.name));
                    self.spawn_declared_install(decl);
                }
                true
            }
            _ => return Some(action),
        };
        debug_assert!(_consumed);
        None
    }
}
