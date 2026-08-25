//! Key handling for the search view: field focus cycling, the scope
//! radio popup, results navigation, and the `/` filter session.

use super::Focus;
use super::GlobalSearch;
use super::model::Scope;
use crate::action::Action;
use crate::components::vim_input::{Outcome, SubMode};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

impl GlobalSearch {
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Active `/` filter session captures everything until commit/cancel.
        if self.filtering {
            return match self.filter.handle_key(key) {
                Outcome::Changed => {
                    self.set_filter(self.filter.value());
                    Action::Noop
                }
                Outcome::Submitted => {
                    self.filtering = false; // commit: filter stays applied
                    Action::Noop
                }
                Outcome::Cancelled => {
                    self.set_filter(self.pre_filter.clone());
                    self.filtering = false;
                    Action::Noop
                }
                Outcome::Noop => Action::Noop,
            };
        }

        // Scope radio popup captures keys while open.
        if self.scope_popup {
            return self.scope_popup_key(key);
        }

        if key.code == KeyCode::Tab {
            self.cycle_focus(false);
            return Action::Noop;
        }
        if key.code == KeyCode::BackTab {
            self.cycle_focus(true);
            return Action::Noop;
        }

        match self.focus {
            Focus::Query => match self.query.handle_key(key) {
                Outcome::Submitted => {
                    self.filter.clear();
                    self.filter_value.clear();
                    self.submit()
                }
                Outcome::Cancelled => Action::CloseSearchView,
                _ => Action::Noop,
            },
            Focus::Extension => match self.extension.handle_key(key) {
                Outcome::Submitted => {
                    self.filter.clear();
                    self.filter_value.clear();
                    self.submit()
                }
                Outcome::Cancelled => Action::CloseSearchView,
                _ => Action::Noop,
            },
            Focus::Scope => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => {
                    // Open the popup with the cursor on the active scope.
                    let items = self.scope_items();
                    self.scope_cursor = items
                        .iter()
                        .position(|(s, _)| *s == self.scope)
                        .unwrap_or(0);
                    self.scope_pre_popup = self.scope;
                    self.scope_popup = true;
                    Action::Noop
                }
                // Cycle scopes right on the field (popup not required).
                KeyCode::Char('j') | KeyCode::Down => {
                    self.cycle_scope(1);
                    Action::Noop
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.cycle_scope(-1);
                    Action::Noop
                }
                KeyCode::Esc => Action::CloseSearchView,
                _ => Action::Noop,
            },
            Focus::Results => match key.code {
                // Leader layer works over the search view too (yank,
                // re-search); App routes leader keys while it's up.
                KeyCode::Char(' ') => Action::Leader,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.move_selection(1);
                    self.context_request().unwrap_or(Action::Noop)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.move_selection(-1);
                    self.context_request().unwrap_or(Action::Noop)
                }
                KeyCode::Char('J') => {
                    self.scroll = self.scroll.saturating_add(1);
                    Action::Noop
                }
                KeyCode::Char('K') => {
                    self.scroll = self.scroll.saturating_sub(1);
                    Action::Noop
                }
                KeyCode::Char('/') => {
                    self.pre_filter = self.filter_value.clone();
                    self.filter.set(&self.pre_filter);
                    self.filtering = true;
                    Action::Noop
                }
                KeyCode::Enter => match self.selected_hit() {
                    Some(hit) => Action::OpenSearchHit(hit.clone()),
                    None => Action::Noop,
                },
                // Committed filter? First Esc clears it, second closes.
                KeyCode::Esc if !self.filter_value.is_empty() => {
                    self.set_filter(String::new());
                    Action::Noop
                }
                KeyCode::Esc => Action::CloseSearchView,
                _ => Action::Noop,
            },
        }
    }

    /// Scope popup keys: the radio follows the cursor live (j/k, g/G),
    /// Enter commits by closing, Esc reverts to the pre-popup scope.
    fn scope_popup_key(&mut self, key: KeyEvent) -> Action {
        let items = self.scope_items();
        match key.code {
            KeyCode::Esc => {
                self.scope = self.scope_pre_popup;
                self.scope_popup = false;
                Action::Noop
            }
            KeyCode::Enter => {
                self.scope_popup = false;
                Action::Noop
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_scope_cursor(next_enabled(&items, self.scope_cursor, 1));
                Action::Noop
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_scope_cursor(next_enabled(&items, self.scope_cursor, -1));
                Action::Noop
            }
            KeyCode::Char('g') => {
                self.move_scope_cursor(next_enabled(&items, items.len() - 1, 1));
                Action::Noop
            }
            KeyCode::Char('G') => {
                self.move_scope_cursor(next_enabled(&items, 0, -1));
                Action::Noop
            }
            _ => Action::Noop,
        }
    }

    /// Move the popup cursor and apply the scope it lands on.
    fn move_scope_cursor(&mut self, idx: usize) {
        self.scope_cursor = idx;
        if let Some((scope, true)) = self.scope_items().get(idx).copied() {
            self.scope = scope;
        }
    }

    fn cycle_focus(&mut self, reverse: bool) {
        let idx = super::FOCUS_ORDER
            .iter()
            .position(|f| *f == self.focus)
            .unwrap_or(0);
        let len = super::FOCUS_ORDER.len();
        let next = if reverse {
            (idx + len - 1) % len
        } else {
            (idx + 1) % len
        };
        self.focus = super::FOCUS_ORDER[next];
        // Focusing a text field always lands in INSERT (plans/0001 §1).
        match self.focus {
            Focus::Query => self.query.submode = SubMode::Insert,
            Focus::Extension => self.extension.submode = SubMode::Insert,
            _ => {}
        }
    }

    /// Move to the next/previous enabled scope without the popup.
    fn cycle_scope(&mut self, delta: i32) {
        let items = self.scope_items();
        let from = items
            .iter()
            .position(|(s, _)| *s == self.scope)
            .unwrap_or(0);
        let idx = next_enabled(&items, from, delta);
        if let Some((scope, true)) = items.get(idx).copied() {
            self.scope = scope;
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.visible().len() as i32;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).clamp(0, len - 1) as usize;
    }

    fn set_filter(&mut self, value: String) {
        self.filter_value = value;
        self.clamp_selection();
    }

    pub(super) fn clamp_selection(&mut self) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    fn submit(&self) -> Action {
        Action::GlobalSearchSubmitted {
            kind: self.kind,
            query: self.query.value(),
            scope: self.scope_label(),
            extension: self.extension.value(),
        }
    }
}

/// Next enabled radio index, wrapping; skips disabled items.
fn next_enabled(items: &[(Scope, bool)], from: usize, delta: i32) -> usize {
    let len = items.len() as i32;
    let mut idx = from as i32;
    for _ in 0..len {
        idx = (idx + delta).rem_euclid(len);
        if items[idx as usize].1 {
            return idx as usize;
        }
    }
    from
}
