//! Modal text field shared by every input in the app (PLAN.md §2).
//! Focus lands in INSERT; Esc → NORMAL with h/l, 0/$, x; i/a → INSERT.
//! Enter (from INSERT) submits.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubMode {
    Insert,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Noop,
    Changed,
    Submitted,
    /// Esc pressed while already in NORMAL — the owner decides what
    /// dismissal means (close popup / exit SEARCHING).
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct VimInput {
    chars: Vec<char>,
    /// Cursor as a char index; in Insert it sits between chars
    /// (0..=len), in Normal it sits on a char (0..len-1).
    cursor: usize,
    pub submode: SubMode,
    /// Modal inputs (popup query) get Esc→NORMAL. Transient inputs
    /// (`/` filter lines) cancel directly on Esc, like vim's `/`.
    modal: bool,
}

impl Default for VimInput {
    fn default() -> Self {
        Self::new()
    }
}

impl VimInput {
    pub fn new() -> Self {
        Self {
            chars: Vec::new(),
            cursor: 0,
            submode: SubMode::Insert,
            modal: true,
        }
    }

    /// Single-stroke input: Esc cancels instead of entering NORMAL.
    pub fn transient() -> Self {
        Self {
            modal: false,
            ..Self::new()
        }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn set(&mut self, value: &str) {
        self.chars = value.chars().collect();
        self.cursor = self.chars.len();
        self.submode = SubMode::Insert;
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    /// Cursor position in chars, for rendering a cursor overlay.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match self.submode {
            SubMode::Insert => self.insert_key(key),
            SubMode::Normal => self.normal_key(key),
        }
    }

    fn insert_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Char(c) => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
                Outcome::Changed
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.chars.remove(self.cursor);
                }
                Outcome::Changed
            }
            KeyCode::Delete => {
                if self.cursor < self.chars.len() {
                    self.chars.remove(self.cursor);
                }
                Outcome::Changed
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                Outcome::Noop
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.chars.len());
                Outcome::Noop
            }
            KeyCode::Home => {
                self.cursor = 0;
                Outcome::Noop
            }
            KeyCode::End => {
                self.cursor = self.chars.len();
                Outcome::Noop
            }
            KeyCode::Enter => Outcome::Submitted,
            KeyCode::Esc => {
                if !self.modal {
                    return Outcome::Cancelled;
                }
                self.submode = SubMode::Normal;
                if self.cursor > 0 && self.cursor == self.chars.len() {
                    self.cursor -= 1;
                }
                Outcome::Noop
            }
            _ => Outcome::Noop,
        }
    }

    fn normal_key(&mut self, key: KeyEvent) -> Outcome {
        let len = self.chars.len();
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                Outcome::Noop
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if len > 0 {
                    self.cursor = (self.cursor + 1).min(len - 1);
                }
                Outcome::Noop
            }
            KeyCode::Char('0') | KeyCode::Home => {
                self.cursor = 0;
                Outcome::Noop
            }
            KeyCode::Char('$') | KeyCode::End => {
                if len > 0 {
                    self.cursor = len - 1;
                }
                Outcome::Noop
            }
            KeyCode::Char('x') => {
                if self.cursor < len {
                    self.chars.remove(self.cursor);
                    if self.cursor >= self.chars.len() && self.cursor > 0 {
                        self.cursor -= 1;
                    }
                }
                Outcome::Changed
            }
            KeyCode::Char('i') => {
                self.submode = SubMode::Insert;
                Outcome::Noop
            }
            KeyCode::Char('a') => {
                self.cursor = (self.cursor + 1).min(len);
                self.submode = SubMode::Insert;
                Outcome::Noop
            }
            KeyCode::Char('A') => {
                self.cursor = len;
                self.submode = SubMode::Insert;
                Outcome::Noop
            }
            KeyCode::Esc => Outcome::Cancelled,
            _ => Outcome::Noop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn types_in_insert_then_esc_to_normal() {
        let mut input = VimInput::new();
        input.handle_key(key(KeyCode::Char('a')));
        input.handle_key(key(KeyCode::Char('b')));
        assert_eq!(input.value(), "ab");
        assert_eq!(input.submode, SubMode::Insert);

        input.handle_key(key(KeyCode::Esc));
        assert_eq!(input.submode, SubMode::Normal);
        // typing no longer inserts
        input.handle_key(key(KeyCode::Char('z')));
        assert_eq!(input.value(), "ab");
    }

    #[test]
    fn normal_motions_and_x() {
        let mut input = VimInput::new();
        input.set("hello");
        input.handle_key(key(KeyCode::Esc));
        input.handle_key(key(KeyCode::Char('0')));
        assert_eq!(input.cursor(), 0);
        input.handle_key(key(KeyCode::Char('x')));
        assert_eq!(input.value(), "ello");
        input.handle_key(key(KeyCode::Char('$')));
        assert_eq!(input.cursor(), 3);
        input.handle_key(key(KeyCode::Char('h')));
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn esc_in_normal_cancels() {
        let mut input = VimInput::new();
        input.handle_key(key(KeyCode::Esc));
        assert_eq!(input.handle_key(key(KeyCode::Esc)), Outcome::Cancelled);
    }

    #[test]
    fn enter_submits_from_insert() {
        let mut input = VimInput::new();
        assert_eq!(input.handle_key(key(KeyCode::Enter)), Outcome::Submitted);
    }

    #[test]
    fn transient_input_esc_cancels_without_normal_mode() {
        let mut input = VimInput::transient();
        input.handle_key(key(KeyCode::Char('x')));
        assert_eq!(input.handle_key(key(KeyCode::Esc)), Outcome::Cancelled);
        assert_eq!(input.submode, SubMode::Insert);
    }

    #[test]
    fn backspace_at_start_is_safe() {
        let mut input = VimInput::new();
        input.handle_key(key(KeyCode::Backspace));
        assert_eq!(input.value(), "");
    }
}
