//! Key handling for the search view: field focus cycling, the scope
//! radio popup, results navigation, and the `/` filter session.

use super::Focus;
use super::GlobalSearch;
use super::model::Scope;
use crate::action::Action;
use crate::components::vim_input::{Outcome, SubMode};
use crate::keymap::{SearchCommand, SearchContext, search_command};
use ratatui::crossterm::event::KeyEvent;

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

        if search_command(SearchContext::Fields, key) == Some(SearchCommand::NextField) {
            self.cycle_focus(false);
            return Action::Noop;
        }
        if search_command(SearchContext::Fields, key) == Some(SearchCommand::PreviousField) {
            self.cycle_focus(true);
            return Action::Noop;
        }

        // The expanded file pane owns the keyboard while it holds
        // focus (plans/0012 M2): j/k walk lines, `/` finds in the
        // file, Enter opens the editor, Esc/h folds back.
        if self.expanded.is_some() && self.focus == super::Focus::Results {
            return self.file_pane_key(key);
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
            Focus::Scope => match search_command(SearchContext::Scope, key) {
                Some(SearchCommand::Open) => {
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
                Some(SearchCommand::Next) => {
                    self.cycle_scope(1);
                    Action::Noop
                }
                Some(SearchCommand::Previous) => {
                    self.cycle_scope(-1);
                    Action::Noop
                }
                Some(SearchCommand::Cancel) => Action::CloseSearchView,
                _ => Action::Noop,
            },
            // plans/0012 M3: the chip row. h/l walk chips (a one-row
            // list), Enter/Space toggles the chip under the cursor —
            // selecting commits the facet, toggling the active chip
            // restores the full accumulated set. Rows live in
            // `keymap::search_facets`.
            Focus::Facets => match search_command(SearchContext::Facets, key) {
                Some(SearchCommand::Left) => {
                    self.move_facet_cursor(-1);
                    Action::Noop
                }
                Some(SearchCommand::Right) => {
                    self.move_facet_cursor(1);
                    Action::Noop
                }
                Some(SearchCommand::Toggle) => {
                    self.toggle_facet();
                    Action::Noop
                }
                // Same Esc ladder as the results pane: peel the
                // committed filters (text, then facet) before closing.
                Some(SearchCommand::Cancel) => self.clear_committed_or_close(),
                _ => Action::Noop,
            },
            Focus::Results => match search_command(SearchContext::Results, key) {
                // Leader layer works over the search view too (yank,
                // re-search); App routes leader keys while it's up.
                Some(SearchCommand::Leader) => Action::Leader,
                Some(SearchCommand::Next) => {
                    self.move_selection(1);
                    self.context_request().unwrap_or(Action::Noop)
                }
                Some(SearchCommand::Previous) => {
                    self.move_selection(-1);
                    self.context_request().unwrap_or(Action::Noop)
                }
                Some(SearchCommand::ScrollDown) => {
                    self.scroll = self.scroll.saturating_add(1);
                    Action::Noop
                }
                Some(SearchCommand::ScrollUp) => {
                    self.scroll = self.scroll.saturating_sub(1);
                    Action::Noop
                }
                Some(SearchCommand::Filter) => {
                    self.pre_filter = self.filter_value.clone();
                    self.filter.set(&self.pre_filter);
                    self.filtering = true;
                    Action::Noop
                }
                // Enter expands the hit's whole file into the results
                // area (plans/0012 M2); Enter again opens the editor.
                Some(SearchCommand::Open) => match self.selected_hit().cloned() {
                    Some(hit) => self.expand_hit(&hit),
                    None => Action::Noop,
                },
                // Committed filters peel one Esc at a time: the `/`
                // text filter first, then the committed facet
                // (plans/0012 M3); the last Esc closes.
                Some(SearchCommand::Cancel) if !self.filter_value.is_empty() => {
                    self.set_filter(String::new());
                    Action::Noop
                }
                Some(SearchCommand::Cancel) if self.facet.is_some() => {
                    self.facet = None;
                    self.clamp_selection();
                    Action::Noop
                }
                Some(SearchCommand::Cancel) => Action::CloseSearchView,
                _ => Action::Noop,
            },
        }
    }

    /// Expanded file pane keys (plans/0012 M2). The re-used `Preview`
    /// owns line movement and the find session; this maps keys onto
    /// it and folds the pane back on Esc/h. Rows live in
    /// `keymap::search_file` — hints derive from there.
    fn file_pane_key(&mut self, key: KeyEvent) -> Action {
        if self.finding {
            match self.find_input.handle_key(key) {
                Outcome::Changed => {
                    if let Some(expanded) = &mut self.expanded {
                        expanded.preview.update_find(self.find_input.value());
                    }
                }
                Outcome::Submitted => self.finding = false,
                Outcome::Cancelled => {
                    if let Some(expanded) = &mut self.expanded {
                        expanded.preview.cancel_find();
                    }
                    self.finding = false;
                }
                Outcome::Noop => {}
            }
            return Action::Noop;
        }
        let Some(expanded) = &mut self.expanded else {
            return Action::Noop;
        };
        // The motion grammar resolves its prefixes first (notably zb),
        // then named commands are looked up in the same table as the hints.
        if expanded.preview.motion_key(key) {
            return Action::Noop;
        }
        match search_command(SearchContext::File, key) {
            Some(SearchCommand::Visual) => expanded.preview.toggle_visual(),
            Some(SearchCommand::Copy) => return Action::PreviewCopy,
            Some(SearchCommand::Yank) => return Action::LeaderYank,
            Some(SearchCommand::CommandLine) => return Action::CommandLine,
            Some(SearchCommand::Blame) => return Action::BlameToggle,
            Some(SearchCommand::Leader) => return Action::Leader,
            Some(SearchCommand::Next) => expanded.preview.move_cursor(1),
            Some(SearchCommand::Previous) => expanded.preview.move_cursor(-1),
            Some(SearchCommand::Filter) => {
                if expanded.preview.findable() {
                    expanded.preview.begin_find();
                    self.find_input.clear();
                    self.finding = true;
                }
            }
            Some(SearchCommand::FindNext) => {
                expanded.preview.find_step(1);
            }
            Some(SearchCommand::FindPrevious) => {
                expanded.preview.find_step(-1);
            }
            Some(SearchCommand::Open) => return Action::OpenSearchHit(expanded.hit.clone()),
            Some(SearchCommand::Cancel) if expanded.preview.clear_visual() => {}
            Some(SearchCommand::Cancel | SearchCommand::Fold) => self.collapse(),
            _ => {}
        }
        Action::Noop
    }

    /// Scope popup keys: the radio follows the cursor live (j/k, g/G),
    /// Enter commits by closing, Esc reverts to the pre-popup scope.
    fn scope_popup_key(&mut self, key: KeyEvent) -> Action {
        let items = self.scope_items();
        match search_command(SearchContext::ScopePopup, key) {
            Some(SearchCommand::Cancel) => {
                self.scope = self.scope_pre_popup;
                self.scope_popup = false;
                Action::Noop
            }
            Some(SearchCommand::Open) => {
                self.scope_popup = false;
                Action::Noop
            }
            Some(SearchCommand::Next) => {
                self.move_scope_cursor(next_enabled(&items, self.scope_cursor, 1));
                Action::Noop
            }
            Some(SearchCommand::Previous) => {
                self.move_scope_cursor(next_enabled(&items, self.scope_cursor, -1));
                Action::Noop
            }
            Some(SearchCommand::First) => {
                self.move_scope_cursor(next_enabled(&items, items.len() - 1, 1));
                Action::Noop
            }
            Some(SearchCommand::Last) => {
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
        let step = |idx: usize| {
            if reverse {
                (idx + len - 1) % len
            } else {
                (idx + 1) % len
            }
        };
        let mut next = step(idx);
        // The chip row only exists once hits hold a facet — cycle
        // past it when there's nothing to focus.
        while super::FOCUS_ORDER[next] == Focus::Facets && self.facets().is_empty() {
            next = step(next);
        }
        self.focus = super::FOCUS_ORDER[next];
        // Focusing a text field always lands in INSERT (plans/0001 §1).
        match self.focus {
            Focus::Query => self.query.submode = SubMode::Insert,
            Focus::Extension => self.extension.submode = SubMode::Insert,
            // Land on the committed chip when there is one.
            Focus::Facets => self.snap_facet_cursor(),
            _ => {}
        }
    }

    /// Move the chip cursor, wrapping across the row.
    fn move_facet_cursor(&mut self, delta: i32) {
        let len = self.facets().len();
        if len == 0 {
            return;
        }
        self.facet_cursor = (self.facet_cursor as i32 + delta).rem_euclid(len as i32) as usize;
    }

    /// Toggle the chip under the cursor: selecting commits the facet
    /// (a local filter over the accumulated set), toggling the active
    /// chip clears it — the full set comes back.
    fn toggle_facet(&mut self) {
        let Some(id) = self.facets().get(self.facet_cursor).map(|c| c.id.clone()) else {
            return;
        };
        self.facet = if self.facet.as_ref() == Some(&id) {
            None
        } else {
            Some(id)
        };
        self.clamp_selection();
    }

    /// Land the chip cursor on the committed facet when there is one.
    fn snap_facet_cursor(&mut self) {
        if let Some(id) = &self.facet
            && let Some(idx) = self.facets().iter().position(|c| &c.id == id)
        {
            self.facet_cursor = idx;
        }
    }

    /// Esc from the results pane or the chip row: peel the committed
    /// filters (text, then facet) before closing the view.
    fn clear_committed_or_close(&mut self) -> Action {
        if !self.filter_value.is_empty() {
            self.set_filter(String::new());
            return Action::Noop;
        }
        if self.facet.is_some() {
            self.facet = None;
            self.clamp_selection();
            return Action::Noop;
        }
        Action::CloseSearchView
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

    /// Keep the chip cursor on a chip as the row reshapes (batches
    /// landing, full-set replacement).
    pub(super) fn clamp_facet_cursor(&mut self) {
        let len = self.facets().len();
        if len == 0 {
            self.facet_cursor = 0;
        } else if self.facet_cursor >= len {
            self.facet_cursor = len - 1;
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
