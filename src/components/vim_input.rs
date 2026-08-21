//! Modal text field shared by every input in the app (PLAN.md §2).
//! Focus lands in INSERT; Esc → NORMAL with h/l, 0/$, x; i/a → INSERT.
//! Enter (from INSERT) submits.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
    /// Prefilled value (resume flow): the first edit replaces it, like
    /// a vim cmdline prefill. Enter submits it as-is.
    replace_on_edit: bool,
    /// `d` pressed in NORMAL; the next motion (w/e/b) deletes, `d`
    /// again clears the line, anything else cancels.
    pending_delete: bool,
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
            replace_on_edit: false,
            pending_delete: false,
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
        self.replace_on_edit = false;
    }
    /// Seed with a replaceable value (resume prefill): typing clears it
    /// first; Enter submits it unchanged.
    pub fn prefill(&mut self, value: &str) {
        self.set(value);
        self.replace_on_edit = true;
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
        // Prefilled input: the first edit replaces the seed value.
        let is_edit = matches!(
            (key.code, key.modifiers),
            (KeyCode::Char(_), KeyModifiers::NONE | KeyModifiers::SHIFT)
                | (KeyCode::Backspace, _)
                | (KeyCode::Delete, _)
                | (KeyCode::Char('w'), KeyModifiers::CONTROL)
                | (KeyCode::Char('u'), KeyModifiers::CONTROL)
        );
        if self.replace_on_edit && is_edit {
            self.chars.clear();
            self.cursor = 0;
            self.replace_on_edit = false;
        }
        match key.code {
            // Ctrl+W / Ctrl+U: word-back / line-start deletes (the
            // line-editing conveniences people expect from vimish UIs).
            KeyCode::Char('w') if key.modifiers == KeyModifiers::CONTROL => {
                let from = word_start_back(&self.chars, self.cursor);
                self.chars.drain(from..self.cursor);
                self.cursor = from;
                Outcome::Changed
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                self.chars.drain(..self.cursor);
                self.cursor = 0;
                Outcome::Changed
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
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
        let pending = std::mem::take(&mut self.pending_delete);
        // A pending `d` resolves on w/e/b (arms below) or d (clear
        // line); anything else cancels it silently, like vim.
        if pending && key.code == KeyCode::Char('d') {
            self.chars.clear();
            self.cursor = 0;
            return Outcome::Changed;
        }
        if pending && !matches!(key.code, KeyCode::Char('w' | 'e' | 'b')) {
            return Outcome::Noop;
        }
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
            KeyCode::Char('d') => {
                self.pending_delete = true; // next: w/e/b delete, dd clears
                Outcome::Noop
            }
            KeyCode::Char('w') => {
                if pending {
                    // dw: through the next run start (or end of line).
                    let to = word_start_fwd(&self.chars, self.cursor);
                    self.chars.drain(self.cursor..to);
                    self.clamp_cursor();
                    Outcome::Changed
                } else {
                    let to = word_start_fwd(&self.chars, self.cursor);
                    if len > 0 {
                        self.cursor = to.min(len - 1);
                    }
                    Outcome::Noop
                }
            }
            KeyCode::Char('e') => {
                if pending {
                    let to = word_end_fwd(&self.chars, self.cursor);
                    self.chars
                        .drain(self.cursor..=(to.min(len.saturating_sub(1))));
                    self.clamp_cursor();
                    Outcome::Changed
                } else {
                    self.cursor = word_end_fwd(&self.chars, self.cursor);
                    Outcome::Noop
                }
            }
            KeyCode::Char('b') => {
                if pending {
                    let from = word_start_back(&self.chars, self.cursor);
                    self.chars.drain(from..self.cursor);
                    self.cursor = from;
                    Outcome::Changed
                } else {
                    self.cursor = word_start_back(&self.chars, self.cursor);
                    Outcome::Noop
                }
            }
            KeyCode::Char('I') => {
                self.cursor = 0;
                self.submode = SubMode::Insert;
                Outcome::Noop
            }
            KeyCode::Char('D') => {
                self.chars.truncate(self.cursor);
                Outcome::Changed
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

impl VimInput {
    /// After a destructive edit: Normal cursors sit ON a char.
    fn clamp_cursor(&mut self) {
        let len = self.chars.len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }
}

/// Word-motion classes: word chars (alnum + `_`), whitespace, and the
/// punctuation runs between them (vim's `word`, not `WORD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Punct
    }
}

/// `w`: start of the run after index `i`.
fn word_start_fwd(chars: &[char], i: usize) -> usize {
    let n = chars.len();
    if i >= n {
        return n;
    }
    let mut j = i;
    if class(chars[j]) != Class::Space {
        let c = class(chars[j]);
        while j < n && class(chars[j]) == c {
            j += 1;
        }
    }
    while j < n && class(chars[j]) == Class::Space {
        j += 1;
    }
    j
}

/// `e`: end (last char index) of the run at/after `i`.
fn word_end_fwd(chars: &[char], i: usize) -> usize {
    let n = chars.len();
    if n == 0 {
        return 0;
    }
    let mut j = i.min(n - 1);
    // Already at a run's end? Step to the next run first.
    if class(chars[j]) != Class::Space && (j + 1 == n || class(chars[j + 1]) != class(chars[j])) {
        j += 1;
        while j < n && class(chars[j]) == Class::Space {
            j += 1;
        }
        if j >= n {
            return n - 1;
        }
    }
    while j < n && class(chars[j]) == Class::Space {
        j += 1;
    }
    let c = class(chars[j]);
    while j + 1 < n && class(chars[j + 1]) == c {
        j += 1;
    }
    j
}

/// `b` / Ctrl+W: start of the run before index `i`. When `i` sits in
/// whitespace, only the whitespace tail is covered (shell-style).
fn word_start_back(chars: &[char], i: usize) -> usize {
    if i == 0 {
        return 0;
    }
    let mut j = i - 1;
    if class(chars[j]) == Class::Space {
        while j > 0 && class(chars[j - 1]) == Class::Space {
            j -= 1;
        }
        return j;
    }
    let c = class(chars[j]);
    while j > 0 && class(chars[j - 1]) == c {
        j -= 1;
    }
    j
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

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Input with `value` typed, switched to NORMAL on the last char.
    fn normal_at_end(value: &str) -> VimInput {
        let mut input = VimInput::new();
        input.set(value);
        input.handle_key(key(KeyCode::Esc));
        input
    }

    #[test]
    fn w_e_b_walk_word_runs() {
        let mut i = normal_at_end("foo bar-baz qux");
        // NORMAL cursor starts on the last char; walk back first.
        i.handle_key(key(KeyCode::Char('0')));
        // "foo bar-baz qux"
        //  0123456789…
        i.handle_key(key(KeyCode::Char('w')));
        assert_eq!(i.cursor(), 4, "w → next word");
        i.handle_key(key(KeyCode::Char('w')));
        assert_eq!(i.cursor(), 7, "w → punctuation run");
        i.handle_key(key(KeyCode::Char('w')));
        assert_eq!(i.cursor(), 8, "w → baz");
        i.handle_key(key(KeyCode::Char('b')));
        assert_eq!(i.cursor(), 7, "b → punctuation");
        i.handle_key(key(KeyCode::Char('b')));
        assert_eq!(i.cursor(), 4, "b → bar");
        i.handle_key(key(KeyCode::Char('0')));
        i.handle_key(key(KeyCode::Char('e')));
        assert_eq!(i.cursor(), 2, "e → end of foo");
        i.handle_key(key(KeyCode::Char('e')));
        assert_eq!(i.cursor(), 6, "e → end of bar");
    }

    #[test]
    fn d_word_deletes() {
        let mut i = normal_at_end("foo bar baz");
        i.handle_key(key(KeyCode::Char('0')));
        i.handle_key(key(KeyCode::Char('d')));
        i.handle_key(key(KeyCode::Char('w')));
        assert_eq!(i.value(), "bar baz", "dw deletes word + spaces");
        i.handle_key(key(KeyCode::Char('0')));
        i.handle_key(key(KeyCode::Char('d')));
        i.handle_key(key(KeyCode::Char('e')));
        assert_eq!(i.value(), " baz", "de deletes through word end");
        let mut i = normal_at_end("foo bar baz");
        // NORMAL cursor is on the last char; db from there.
        i.handle_key(key(KeyCode::Char('d')));
        i.handle_key(key(KeyCode::Char('b')));
        assert_eq!(i.value(), "foo bar z", "db deletes the previous word");
        // d then a non-motion cancels silently.
        let mut i = normal_at_end("keep me");
        i.handle_key(key(KeyCode::Char('d')));
        i.handle_key(key(KeyCode::Char('q')));
        assert_eq!(i.value(), "keep me");
    }

    #[test]
    fn dd_clears_and_shift_d_truncates() {
        let mut i = normal_at_end("gone soon");
        i.handle_key(key(KeyCode::Char('d')));
        i.handle_key(key(KeyCode::Char('d')));
        assert_eq!(i.value(), "", "dd clears the line");
        assert_eq!(i.cursor(), 0);

        let mut i = normal_at_end("keep drop");
        i.handle_key(key(KeyCode::Char('0')));
        i.handle_key(key(KeyCode::Char('w'))); // on 'd'
        i.handle_key(key(KeyCode::Char('D')));
        assert_eq!(i.value(), "keep ", "D deletes to end of line");
    }

    #[test]
    fn ctrl_w_deletes_word_back_in_insert() {
        let mut i = VimInput::new();
        i.set("foo bar");
        i.handle_key(ctrl('w'));
        assert_eq!(i.value(), "foo ");
        assert_eq!(i.cursor(), 4);
        // Trailing whitespace goes first, one step at a time.
        let mut i = VimInput::new();
        i.set("foo   ");
        i.handle_key(ctrl('w'));
        assert_eq!(i.value(), "foo");
        i.handle_key(ctrl('w'));
        assert_eq!(i.value(), "");
        // Ctrl+W does NOT type a literal w.
        let mut i = VimInput::new();
        i.handle_key(ctrl('w'));
        assert_eq!(i.value(), "");
    }

    #[test]
    fn ctrl_u_clears_to_line_start() {
        let mut i = VimInput::new();
        i.set("hello world");
        i.handle_key(ctrl('u'));
        assert_eq!(i.value(), "");
        assert_eq!(i.cursor(), 0);
    }

    #[test]
    fn shift_i_inserts_at_line_start() {
        let mut i = normal_at_end("abc");
        i.handle_key(key(KeyCode::Char('I')));
        assert_eq!(i.cursor(), 0);
        assert_eq!(i.submode, SubMode::Insert);
        i.handle_key(key(KeyCode::Char('X')));
        assert_eq!(i.value(), "Xabc");
    }
}
