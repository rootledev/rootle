//! Blame state and run-margin integration with the shared file preview.

use super::Browser;

/// Blame lens state (plans/0016 M1c): ranges for one path, fetched on
/// demand; the marks live in the Preview. Shared with the search
/// view's expanded pane (0019 parity).
pub(crate) struct BlameState {
    pub(crate) path: String,
    pub(crate) ranges: Vec<rootle_provider::BlameRange>,
    pub(crate) loading: bool,
}

impl Browser {
    /// `␣ p b` off: drop the lens. (On is the app's: it fetches
    /// ranges via `blame_request` and they land in `blame_store`.)
    pub fn clear_blame(&mut self) {
        self.blame = None;
        self.preview.set_blame(None);
    }

    /// Toggle on: ranges already fetched for the file under preview
    /// apply immediately; otherwise the app spawns the fetch. Returns
    /// the (repo-path) the marks belong to when a fetch is needed.
    pub fn blame_toggle_on(&mut self) -> bool {
        if self.preview.text_line_count() == 0 {
            return false;
        }
        let apply = matches!(&self.blame, Some(b) if !b.loading);
        if apply {
            self.blame_apply();
        }
        true
    }

    /// The path a blame fetch should cover, if one is needed.
    pub fn blame_needed_for(&self) -> Option<String> {
        if self.preview.text_line_count() == 0 {
            return None;
        }
        match &self.blame {
            Some(b) if b.loading => None, // in flight
            Some(_) => None,              // loaded — blame_apply covers it
            None => self.selected_file().map(|(p, _)| p),
        }
    }

    pub fn blame_mark_loading(&mut self, path: String) {
        self.blame = Some(BlameState {
            path,
            ranges: Vec::new(),
            loading: true,
        });
    }

    /// Ranges landed (identity-checked by the caller); apply when the
    /// lens is open on this path.
    pub fn blame_store(&mut self, path: String, ranges: Vec<rootle_provider::BlameRange>) {
        let active = self.blame_awaiting(&path);
        self.blame = Some(BlameState {
            path,
            ranges,
            loading: false,
        });
        if active {
            self.blame_apply();
        }
    }

    /// Ranges → per-line run marks on the preview (v1.5 shape:
    /// coalesced 1-based inclusive ranges; run starts carry the mark).
    fn blame_apply(&mut self) {
        let Some(b) = &self.blame else { return };
        let lines = self.preview.text_line_count();
        if lines == 0 {
            return;
        }
        let mut marks: Vec<Option<crate::components::preview::BlameMark>> = vec![None; lines];
        for r in &b.ranges {
            let start = (r.start_line as usize).saturating_sub(1);
            if start < lines {
                marks[start] = Some(crate::components::preview::BlameMark {
                    sha: r.sha.chars().take(7).collect(),
                    author: r.author.clone(),
                });
            }
        }
        self.preview.set_blame(Some(marks));
    }

    /// Enter on a blame line: the sha the margin names for that line —
    /// the history lens opens positioned at it. None outside blame.
    pub fn blame_line_sha(&self) -> Option<String> {
        if !self.preview.blaming() {
            return None;
        }
        let b = self.blame.as_ref()?;
        if b.loading {
            return None;
        }
        let line = self.preview_line().unwrap_or(1) as usize;
        b.ranges
            .iter()
            .find(|r| r.start_line as usize <= line && line <= r.end_line as usize)
            .map(|r| r.sha.clone())
    }
}
