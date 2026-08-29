//! Vim vertical motions + the line cursor/viewport engine
//! (moved from preview.rs, plans/0021 M2 — a pure move).

use super::Preview;

impl Preview {
    /// Move the line cursor (J/K). No-op for cursorless content
    /// (dirs, binaries, empty).
    pub fn move_cursor(&mut self, delta: i32) {
        if self.line_count == 0 {
            return;
        }
        self.cursor = self
            .cursor
            .saturating_add_signed(delta as i16)
            .min(self.line_count - 1);
    }

    /// Drop the cursor onto a 1-based line (hit expand, plans/0012
    /// M2): clamped to the content, scroll follows on the next
    /// render. No-op for cursorless content; `line = 0` (unknown
    /// anchor) keeps the top.
    pub fn set_cursor_line(&mut self, line: u32) {
        if self.line_count == 0 || line == 0 {
            return;
        }
        let target = line.saturating_sub(1).min(u32::from(self.line_count - 1));
        self.cursor = target as u16;
    }

    /// Current cursor line, 1-based — what `␣ y` anchors to.
    pub fn line(&self) -> Option<u32> {
        (self.line_count > 0).then(|| u32::from(self.cursor) + 1)
    }

    /// The pending count, cleared. None = no digits typed.
    fn take_count(&mut self) -> Option<usize> {
        let n = if self.motion_count.is_empty() {
            None
        } else {
            self.motion_count.parse().ok()
        };
        self.motion_count.clear();
        n
    }

    fn goto_line(&mut self, line_1based: usize) {
        let max = self.line_count as usize;
        self.cursor = (line_1based.max(1).min(max) - 1) as u16;
    }

    /// One key of the motion set. Consumed keys return true; anything
    /// else falls through to the caller's named actions. Counts and a
    /// pending head reset on any non-motion key.
    pub fn motion_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> bool {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        if self.line_count == 0 {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(c) if !ctrl && c.is_ascii_digit() && self.motion_pending.is_none() => {
                if self.motion_count.is_empty() && c == '0' {
                    return false; // 0 is a column motion — out of scope
                }
                self.motion_count.push(c);
                true
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                let n = self.take_count().unwrap_or(1) as i32;
                self.motion_pending = None;
                self.move_cursor(n);
                true
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                let n = self.take_count().unwrap_or(1) as i32;
                self.motion_pending = None;
                self.move_cursor(-n);
                true
            }
            KeyCode::Char('g') if !ctrl => {
                if self.motion_pending == Some('g') {
                    let n = self.take_count().unwrap_or(1);
                    self.motion_pending = None;
                    self.goto_line(n);
                } else {
                    self.motion_pending = Some('g');
                }
                true
            }
            KeyCode::Char('G') if !ctrl => {
                let n = self.take_count().unwrap_or(self.line_count as usize);
                self.motion_pending = None;
                self.goto_line(n);
                true
            }
            KeyCode::Char('d') if ctrl => {
                let n = (self.viewport as usize / 2).max(1) * self.take_count().unwrap_or(1);
                self.move_cursor(n as i32);
                true
            }
            KeyCode::Char('u') if ctrl => {
                let n = (self.viewport as usize / 2).max(1) * self.take_count().unwrap_or(1);
                self.move_cursor(-(n as i32));
                true
            }
            KeyCode::Char('f') if ctrl => {
                let n = (self.viewport as usize).max(1) * self.take_count().unwrap_or(1);
                self.move_cursor(n as i32);
                true
            }
            KeyCode::Char('b') if ctrl => {
                let n = (self.viewport as usize).max(1) * self.take_count().unwrap_or(1);
                self.move_cursor(-(n as i32));
                true
            }
            KeyCode::Char('{') if !ctrl => {
                self.motion_pending = None;
                self.take_count();
                let lines = self.plain_lines();
                let mut i = self.cursor as usize;
                // vim-true: off any blank, to the paragraph's first
                // line, then the blank above it.
                while i > 0 && lines[i].trim().is_empty() {
                    i -= 1;
                }
                while i > 0 && !lines[i - 1].trim().is_empty() {
                    i -= 1;
                }
                i = i.saturating_sub(1);
                self.cursor = i as u16;
                true
            }
            KeyCode::Char('}') if !ctrl => {
                self.motion_pending = None;
                self.take_count();
                let lines = self.plain_lines();
                let max = self.line_count as usize;
                let mut i = self.cursor as usize;
                while i + 1 < max && lines[i + 1].trim().is_empty() {
                    i += 1;
                }
                while i + 1 < max && !lines[i + 1].trim().is_empty() {
                    i += 1;
                }
                if i + 1 < max {
                    i += 1; // land on the blank below
                }
                self.cursor = i as u16;
                true
            }
            KeyCode::Char('%') if !ctrl => {
                self.motion_pending = None;
                self.take_count();
                let lines = self.plain_lines();
                if let Some(target) = bracket_match(&lines, self.cursor as usize) {
                    self.cursor = target as u16;
                }
                true
            }
            KeyCode::Char('z') if !ctrl => {
                match self.motion_pending {
                    Some('z') => {
                        // zz: center the cursor line.
                        self.motion_pending = None;
                        self.scroll = self
                            .cursor
                            .saturating_sub(self.viewport / 2)
                            .min(self.line_count.saturating_sub(self.viewport));
                    }
                    _ => self.motion_pending = Some('z'),
                }
                true
            }
            KeyCode::Char('t') if !ctrl && self.motion_pending == Some('z') => {
                self.motion_pending = None;
                // vim's zt pads past EOF rather than not pinning.
                self.scroll = self.cursor;
                true
            }
            KeyCode::Char('b') if !ctrl && self.motion_pending == Some('z') => {
                self.motion_pending = None;
                self.scroll = self
                    .cursor
                    .saturating_add_signed(1)
                    .saturating_sub(self.viewport);
                true
            }
            _ => {
                self.motion_count.clear();
                self.motion_pending = None;
                false
            }
        }
    }
}

/// `%`: the first bracket on the cursor line, matched across lines
/// with nesting depth. Returns the target LINE (vertical motion only).
/// Strings/comments aren't parsed — the tree-sitter upgrade path is
/// plans/0013's grammar set.
fn bracket_match(lines: &[String], line: usize) -> Option<usize> {
    let text = lines.get(line)?;
    // No column to anchor on (vertical motion): the target is the
    // first bracket whose pair is NOT closed on this same line —
    // `fn main() {` means the brace, not the paren.
    let (col, b) = text
        .char_indices()
        .filter(|(_, c)| "(){}[]".contains(*c))
        .find(|(ci, c)| {
            if "([{".contains(*c) {
                !text[*ci + c.len_utf8()..].contains(pairs(*c))
            } else {
                // A closer whose opener sits on this line pairs
                // locally — keep scanning.
                !text[..*ci].contains(pairs(*c))
            }
        })?;
    let (open, close, forward) = match b {
        '(' | '[' | '{' => (b, pairs(b), true),
        _ => (pairs(b), b, false),
    };
    let mut depth = 1i32;
    if forward {
        let mut li = line;
        let mut skip = col + 1;
        while li < lines.len() {
            for (ci, c) in lines[li].char_indices() {
                if ci < skip {
                    continue;
                }
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some(li);
                    }
                }
            }
            skip = 0;
            li += 1;
        }
    } else {
        let mut li = line;
        let mut take_until = col;
        loop {
            for (ci, c) in lines[li].char_indices().rev() {
                if ci >= take_until {
                    continue;
                }
                if c == b {
                    depth += 1;
                } else if c == open {
                    depth -= 1;
                    if depth == 0 {
                        return Some(li);
                    }
                }
            }
            if li == 0 {
                break;
            }
            li -= 1;
            take_until = usize::MAX;
        }
    }
    None
}

/// Matching bracket pairs.
fn pairs(c: char) -> char {
    match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        ')' => '(',
        ']' => '[',
        '}' => '{',
        _ => unreachable!(),
    }
}
