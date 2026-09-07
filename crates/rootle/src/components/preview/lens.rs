//! Pane lenses: the header band, blame marks, and vim visual-lines
//! (moved from preview.rs, plans/0021 M2 — a pure move).

use super::Preview;

#[derive(Debug, Clone)]
pub struct BandContext {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone)]
pub struct BlameMark {
    pub sha: String,
    pub author: String,
}

/// The sha prefix's length inside the shaped band text — the
/// extreme-squeeze fallback text is sha-only (the whole thing).
pub(crate) fn sha_len(text: &str, ctx: &BandContext) -> usize {
    if text.starts_with(&ctx.sha) {
        ctx.sha.len()
    } else {
        text.len()
    }
}

impl Preview {
    /// The blame lens: per-line marks, or None to leave it.
    pub fn set_blame(&mut self, marks: Option<Vec<Option<BlameMark>>>) {
        self.blame = marks;
    }

    /// The header band (always-on for file content): the full path
    /// left; at-commit context right when set (None restores).
    pub fn set_band(&mut self, path: Option<String>, context: Option<BandContext>) {
        self.band_path = path;
        self.band_context = context;
    }

    /// Blame lens active?
    pub fn blaming(&self) -> bool {
        self.blame.is_some()
    }

    /// vim's V (pane-local line visual): toggles the anchor at the
    /// cursor; motions extend the range. No-op on cursorless content.
    pub fn toggle_visual(&mut self) {
        if self.line_count == 0 {
            return;
        }
        self.visual_anchor = match self.visual_anchor {
            Some(_) => None,
            None => Some(self.cursor),
        };
    }

    /// Esc inside the pane: a selection clears first, the caller's
    /// exit follows. Returns true while visual stays/just cleared.
    pub fn clear_visual(&mut self) -> bool {
        self.visual_anchor.take().is_some()
    }

    /// The selected 1-based line range (start, end) while visual is
    /// on; None otherwise.
    pub fn visual_range(&self) -> Option<(u32, u32)> {
        let a = self.visual_anchor?;
        let (lo, hi) = (a.min(self.cursor), a.max(self.cursor));
        Some((u32::from(lo) + 1, u32::from(hi) + 1))
    }
}
