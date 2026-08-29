//! The expanded file pane: its engine accessors and the 0019
//! parity surface (yank target, last-commit band, blame lens)
//! — moved from global_search.rs, plans/0021 M2, a pure move.

use super::{GlobalSearch, Preview, SearchHit, SearchKind};
use crate::components::browser::BlameState;

/// A resolved URL-yank target (0019 parity): the search context's
/// answer to the browser's cursor-anchored yank.
pub struct YankTarget {
    pub repo: String,
    pub path: String,
    pub branch: String,
    pub line: Option<u32>,
    pub end: Option<u32>,
}

/// The expanded full-file pane (plans/0012 M2). The re-used browser
/// `Preview` does the rendering (numbered lines, cursor, gutter,
/// find-in-file); this holds the anchor hit it was opened for.
pub(crate) struct ExpandedFile {
    /// Snapshot of the hit at expand time — repo/sha identify the
    /// blob, `line` is the cursor anchor (refreshed by a lazy context
    /// landing while the blob is still in flight).
    pub(crate) hit: SearchHit,
    /// Set once the file content landed; later anchor refinements are
    /// ignored — the user may already have walked the cursor.
    pub(crate) loaded: bool,
    pub(crate) preview: Preview,
}

impl GlobalSearch {
    /// `Y` in the expanded file pane: its preview's copy target.
    pub fn expanded_copy_target(&mut self) -> Option<(String, usize)> {
        self.expanded
            .as_mut()
            .and_then(|exp| exp.preview.copy_target())
    }

    /// `:42` — jump the expanded file pane to a line (plans/0016 M1).
    /// Returns false when nothing is expanded.
    pub fn expanded_goto_line(&mut self, line: u32) -> bool {
        match &mut self.expanded {
            Some(exp) => {
                exp.preview.set_cursor_line(line);
                true
            }
            None => false,
        }
    }

    /// The expanded hit's branch — the band memo's ref key.
    pub fn expanded_branch(&self) -> Option<String> {
        self.expanded.as_ref().map(|e| e.hit.branch.clone())
    }

    /// The last-commit band for the expanded pane (0019 polish):
    /// applied when the memo lands; the band dresses like the miller
    /// preview's.
    pub fn expanded_set_band(
        &mut self,
        path: &str,
        ctx: Option<crate::components::preview::BandContext>,
    ) {
        if let Some(exp) = &mut self.expanded
            && exp.hit.path == path
        {
            exp.preview.set_band(Some(path.to_string()), ctx);
        }
    }

    /// The yank target for the current context: the expanded pane
    /// anchors to its line cursor (or visual range); otherwise the
    /// selected hit's own line.
    pub fn yank_target(&self) -> Option<YankTarget> {
        if let Some(exp) = &self.expanded {
            let (line, end) = match exp.preview.visual_range() {
                Some((lo, hi)) => (Some(lo), Some(hi)),
                None => (exp.preview.line(), None),
            };
            return Some(YankTarget {
                repo: exp.hit.repo.clone(),
                path: exp.hit.path.clone(),
                branch: exp.hit.branch.clone(),
                line,
                end,
            });
        }
        self.selected_hit().map(|h| YankTarget {
            repo: h.repo.clone(),
            path: h.path.clone(),
            branch: h.branch.clone(),
            line: Some(h.line),
            end: None,
        })
    }

    /// The query text (raw, as typed) — the app chips the expanded
    /// pane's lines with it, the same chip the results list wears.
    pub fn query_text(&self) -> String {
        self.query.value()
    }

    pub fn is_grep(&self) -> bool {
        self.kind == SearchKind::Grep
    }

    /// Blame currently shown in the expanded pane.
    pub fn blame_active(&self) -> bool {
        self.expanded.as_ref().is_some_and(|e| e.preview.blaming())
    }

    /// `b` off: drop the lens.
    pub fn blame_clear(&mut self) {
        self.blame = None;
        if let Some(exp) = &mut self.expanded {
            exp.preview.set_blame(None);
        }
    }

    /// Toggle on: text present, no fetch in flight or loaded. Mirrors
    /// the browser's state machine over the expanded pane's preview.
    pub fn blame_toggle_on(&mut self) -> bool {
        match &self.expanded {
            Some(exp) if exp.preview.text_line_count() > 0 => {
                if matches!(&self.blame, Some(b) if !b.loading) {
                    self.blame_apply();
                }
                true
            }
            _ => false,
        }
    }

    /// The (repo, path, branch) a blame fetch should cover, if needed.
    pub fn blame_needed(&self) -> Option<(String, String, String)> {
        let exp = self.expanded.as_ref()?;
        if exp.preview.text_line_count() == 0 {
            return None;
        }
        match &self.blame {
            None => Some((
                exp.hit.repo.clone(),
                exp.hit.path.clone(),
                exp.hit.branch.clone(),
            )),
            Some(_) => None, // loaded or in flight
        }
    }

    pub fn blame_mark_loading(&mut self, path: String) {
        self.blame = Some(BlameState {
            path,
            ranges: Vec::new(),
            loading: true,
        });
    }

    /// The path an in-flight fetch would fill (event identity check).
    pub fn blame_loading_for(&self, path: &str) -> bool {
        matches!(&self.blame, Some(b) if b.loading && b.path == path)
    }

    /// Ranges landed — apply to the expanded pane when they're for it.
    pub fn blame_store(&mut self, path: String, ranges: Vec<crate::provider::BlameRange>) {
        let active = self.blame_loading_for(&path);
        self.blame = Some(BlameState {
            path,
            ranges,
            loading: false,
        });
        if active {
            self.blame_apply();
        }
    }

    fn blame_apply(&mut self) {
        let Some(b) = &self.blame else { return };
        let Some(exp) = &mut self.expanded else {
            return;
        };
        let lines = exp.preview.text_line_count();
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
        exp.preview.set_blame(Some(marks));
    }
}
