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
            .saturating_add_signed(delta as isize)
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
        self.cursor = (line.saturating_sub(1) as usize).min(self.line_count - 1);
    }

    /// Current cursor line, 1-based — what `␣ y` anchors to.
    pub fn line(&self) -> Option<u32> {
        (self.line_count > 0).then(|| u32::try_from(self.cursor + 1).unwrap_or(u32::MAX))
    }

    /// The pending count, cleared. None = no digits typed.
    fn take_count(&mut self) -> Option<usize> {
        self.motion_count.take()
    }

    fn goto_line(&mut self, line_1based: usize) {
        self.cursor = line_1based.max(1).min(self.line_count) - 1;
    }

    /// One key of the motion set. Consumed keys return true; anything
    /// else falls through to the caller's named actions. Counts and a
    /// pending head reset on any non-motion key.
    pub fn motion_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> bool {
        use crate::keymap::{MotionCommand, MotionPrefix};
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        if self.line_count == 0 {
            return false;
        }
        if let KeyCode::Char(character) = key.code
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && character.is_ascii_digit()
            && self.motion_pending.is_none()
        {
            return self
                .motion_count
                .push(character.to_digit(10).expect("ASCII digit"));
        }
        match crate::keymap::motion(key) {
            Some(MotionCommand::Down) => {
                let count = self.take_count().unwrap_or(1);
                self.motion_pending = None;
                self.move_cursor(count.min(self.line_count).min(i32::MAX as usize) as i32);
            }
            Some(MotionCommand::Up) => {
                let count = self.take_count().unwrap_or(1);
                self.motion_pending = None;
                self.move_cursor(-(count.min(self.line_count).min(i32::MAX as usize) as i32));
            }
            Some(MotionCommand::Start) => {
                if self.motion_pending == Some(MotionPrefix::Start) {
                    let count = self.take_count().unwrap_or(1);
                    self.motion_pending = None;
                    self.goto_line(count);
                } else {
                    self.motion_pending = Some(MotionPrefix::Start);
                }
            }
            Some(MotionCommand::End) => {
                let count = self.take_count().unwrap_or(self.line_count);
                self.motion_pending = None;
                self.goto_line(count);
            }
            Some(
                command @ (MotionCommand::HalfPageDown
                | MotionCommand::HalfPageUp
                | MotionCommand::PageDown
                | MotionCommand::PageUp),
            ) => {
                let page = match command {
                    MotionCommand::HalfPageDown | MotionCommand::HalfPageUp => self.viewport / 2,
                    _ => self.viewport,
                }
                .max(1);
                let count = page
                    .saturating_mul(self.take_count().unwrap_or(1))
                    .min(self.line_count)
                    .min(i32::MAX as usize) as i32;
                self.motion_pending = None;
                self.move_cursor(
                    if matches!(command, MotionCommand::HalfPageUp | MotionCommand::PageUp) {
                        -count
                    } else {
                        count
                    },
                );
            }
            Some(MotionCommand::ParagraphPrevious) => {
                self.motion_pending = None;
                self.take_count();
                let lines = self.plain_lines();
                let mut line = self.cursor;
                while line > 0 && lines[line].trim().is_empty() {
                    line -= 1;
                }
                while line > 0 && !lines[line - 1].trim().is_empty() {
                    line -= 1;
                }
                self.cursor = line.saturating_sub(1);
            }
            Some(MotionCommand::ParagraphNext) => {
                self.motion_pending = None;
                self.take_count();
                let lines = self.plain_lines();
                let mut line = self.cursor;
                let last = self.line_count - 1;
                while line < last && lines[line + 1].trim().is_empty() {
                    line += 1;
                }
                while line < last && !lines[line + 1].trim().is_empty() {
                    line += 1;
                }
                self.cursor = line + usize::from(line < last);
            }
            Some(MotionCommand::Bracket) => {
                self.motion_pending = None;
                self.take_count();
                if let Some(line) = bracket_match(&self.plain_lines(), self.cursor) {
                    self.cursor = line;
                }
            }
            Some(MotionCommand::Align) => {
                if self.motion_pending == Some(MotionPrefix::Align) {
                    self.motion_pending = None;
                    self.scroll = self
                        .cursor
                        .saturating_sub(self.viewport / 2)
                        .min(self.line_count.saturating_sub(self.viewport));
                } else {
                    self.motion_pending = Some(MotionPrefix::Align);
                }
            }
            Some(MotionCommand::AlignTop) if self.motion_pending == Some(MotionPrefix::Align) => {
                self.motion_pending = None;
                self.scroll = self.cursor;
            }
            Some(MotionCommand::AlignBottom)
                if self.motion_pending == Some(MotionPrefix::Align) =>
            {
                self.motion_pending = None;
                self.scroll = self.cursor.saturating_add(1).saturating_sub(self.viewport);
            }
            _ => {
                self.motion_count.clear();
                self.motion_pending = None;
                return false;
            }
        }
        true
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
