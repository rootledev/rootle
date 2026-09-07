//! One incremental `/` session: edit live, Enter keeps, Esc restores.

use crate::components::vim_input::{Outcome, VimInput};
use ratatui::crossterm::event::KeyEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOutcome {
    Unchanged,
    Changed,
    Committed,
    Restored,
}

pub struct ListFilter {
    input: VimInput,
    text: String,
    folded: String,
    baseline: Option<String>,
}

impl Default for ListFilter {
    fn default() -> Self {
        Self {
            input: VimInput::transient(),
            text: String::new(),
            folded: String::new(),
            baseline: None,
        }
    }
}

impl ListFilter {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn active(&self) -> bool {
        self.baseline.is_some()
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn begin(&mut self) {
        if self.active() {
            return;
        }
        self.input.set(&self.text);
        self.baseline = Some(self.text.clone());
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FilterOutcome {
        if !self.active() {
            return FilterOutcome::Unchanged;
        }
        match self.input.handle_key(key) {
            Outcome::Changed => {
                self.set_text(self.input.value());
                FilterOutcome::Changed
            }
            Outcome::Submitted => {
                self.set_text(self.input.value());
                self.baseline = None;
                FilterOutcome::Committed
            }
            Outcome::Cancelled => {
                let baseline = self.baseline.take().unwrap_or_default();
                self.set_text(baseline);
                FilterOutcome::Restored
            }
            Outcome::Noop => FilterOutcome::Unchanged,
        }
    }

    /// Clear a committed filter. True means Esc consumed a filter rather
    /// than dismissing the owning surface.
    pub fn clear(&mut self) -> bool {
        let consumed = self.active() || !self.is_empty();
        self.baseline = None;
        self.input.set("");
        self.set_text(String::new());
        consumed
    }

    pub fn matches(&self, text: &str) -> bool {
        self.folded.is_empty() || text.to_lowercase().contains(&self.folded)
    }

    pub fn visible<T>(&self, items: &[T], matches: impl Fn(&T, &Self) -> bool) -> Vec<usize> {
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches(item, self))
            .map(|(index, _)| index)
            .collect()
    }

    fn set_text(&mut self, text: String) {
        self.folded = text.to_lowercase();
        self.text = text;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn filter_is_live_and_cancel_restores_the_committed_value() {
        let mut filter = ListFilter::default();
        filter.begin();
        filter.handle_key(key(KeyCode::Char('r')));
        assert!(filter.matches("README"));
        assert!(!filter.matches("lib.toml"));
        filter.handle_key(key(KeyCode::Enter));
        filter.begin();
        filter.handle_key(key(KeyCode::Char('s')));
        assert!(!filter.matches("README"));
        filter.handle_key(key(KeyCode::Esc));
        assert_eq!(filter.text(), "r");
        assert!(filter.clear());
        assert!(!filter.clear());
        filter.begin();
        filter.handle_key(key(KeyCode::Char('x')));
        assert_eq!(filter.text(), "x", "reopening cannot revive the old input");
    }
}
