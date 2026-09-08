//! Results for GlobalSearch.

use super::{
    Action, ExpandedFile, Focus, GlobalSearch, Preview, PreviewContent, RENDER_CAP, SearchHit,
    file_title, line_text,
};

impl GlobalSearch {
    /// Hits surviving the committed facet (plans/0012 M3) and the
    /// committed `/` filter (path or preview text, case-insensitive
    /// substring — same rule as Pane::visible). Facet first, then the
    /// filter text; the two compose.
    pub(super) fn visible(&self) -> Vec<&SearchHit> {
        let needle = self.filter_value.to_lowercase();
        self.hits
            .iter()
            .filter(|h| self.facet.as_ref().is_none_or(|f| f.matches(h)))
            .filter(|h| {
                needle.is_empty()
                    || h.path.to_lowercase().contains(&needle)
                    || h.preview
                        .iter()
                        .any(|(_, line)| line_text(line).to_lowercase().contains(&needle))
            })
            .collect()
    }

    pub fn selected_hit(&self) -> Option<&SearchHit> {
        self.visible().get(self.selected).copied()
    }

    /// v1.1 lazy context (plans/0006 §1): the cursor sits on a hit
    /// with a sha but no preview lines — ask for its blob context.
    pub fn context_request(&self) -> Option<Action> {
        let hit = self.selected_hit()?;
        if !hit.preview.is_empty() || hit.sha.is_empty() {
            return None;
        }
        Some(Action::LoadHitContext {
            hit: Box::new(hit.clone()),
            query: self.query_text(),
        })
    }

    /// Expand a hit into the full-file pane (plans/0012 M2). Real
    /// hits (sha) open as a loading placeholder and ask App for the
    /// blob — cache-first, so the lazy context usually warmed it;
    /// mock hits (body, no sha) render inline. The returned action is
    /// the fetch, when one is needed.
    pub(super) fn expand_hit(&mut self, hit: &SearchHit) -> Action {
        let mut preview = Preview::focused();
        let action = if hit.sha.is_empty() {
            preview.set_bytes(&hit.path, hit.body.as_bytes());
            preview.title = file_title(hit);
            preview.set_cursor_line(hit.line);
            Action::Noop
        } else {
            preview.set_file_meta(&hit.path, None, &hit.sha);
            preview.title = file_title(hit);
            Action::LoadHitFile {
                hit: Box::new(hit.clone()),
            }
        };
        self.expanded = Some(ExpandedFile {
            hit: hit.clone(),
            loaded: hit.sha.is_empty(),
            preview,
        });
        action
    }

    /// Fold the file pane back to the results list (Esc/h). The list,
    /// the selection, and its scroll were never touched — collapse
    /// restores the exact view.
    pub(super) fn collapse(&mut self) {
        self.expanded = None;
        self.finding = false;
        self.find_input.clear();
    }

    pub fn start_request(&mut self, request: crate::request::ContentSearchRequest) {
        self.load.start(request);
        self.failure_preview = None;
        self.hits.clear();
        self.dropped = 0;
        self.clipped = false;
        self.index_as_of = None;
        self.client_filtered = 0;
        self.unfiltered.clear();
        self.facet = None;
        self.facet_cursor = 0;
        self.focus = Focus::Results;
        self.selected = 0;
        self.scroll = 0;
        self.filter.clear();
        self.filter_value.clear();
        self.filtering = false;
        self.collapse();
    }

    pub fn update(&mut self, action: &Action) {
        match action {
            Action::GlobalSearchDelta { hits } => {
                self.append_hits(hits.clone());
                self.clamp_facet_cursor(); // chips grew — keep the cursor on one
            }
            Action::GlobalSearchResults {
                hits,
                clipped,
                index,
                client_filtered,
                unfiltered,
            } => {
                self.load.finish(None);
                self.failure_preview = None;
                self.clipped = *clipped || self.dropped > 0;
                self.index_as_of = index.clone();
                self.client_filtered = *client_filtered;
                self.unfiltered = unfiltered.clone();
                // A streamed final is metadata-only (empty hits) — the
                // accumulated set stands. A full set replaces it.
                if !hits.is_empty() {
                    self.hits = hits.clone();
                    self.dropped = 0;
                    self.selected = 0;
                    self.scroll = 0;
                }
                self.clamp_selection();
                self.clamp_facet_cursor();
            }
            Action::GlobalSearchFailed { error } => {
                self.load.finish(Some(error));
                let error = self.load.error.as_ref().expect("failed outcome");
                let mut preview = Preview::focused();
                let partial = if self.hits.is_empty() {
                    "No completed result set.".to_string()
                } else {
                    format!(
                        "{} partial results retained; search is incomplete.",
                        self.hits.len()
                    )
                };
                let retry = error
                    .retry_after_s
                    .map(|seconds| format!("\nProvider retry delay: {seconds}s."))
                    .unwrap_or_default();
                preview.set_text("search failed", format!(
                    "Search failed ({})\n{}\n{partial}{retry}\nEdit the query and submit to retry.",
                    error.kind_name(), error.message,
                ));
                self.failure_preview = Some(preview);
                if self.hits.is_empty() && self.focus == Focus::Results {
                    self.focus = Focus::Error;
                }
            }
            // v1.1 lazy context landed (plans/0006 §1): merge by
            // identity — the hit list may have been filtered/reordered
            // since the fetch started.
            Action::HitContextLoaded {
                repo,
                path,
                sha,
                line,
                preview,
                match_count,
            } => {
                let target = self
                    .hits
                    .iter_mut()
                    .find(|h| &h.repo == repo && &h.path == path && &h.sha == sha);
                if let Some(hit) = target
                    && !preview.is_empty()
                {
                    hit.line = *line;
                    hit.preview = preview.clone();
                    hit.match_count = *match_count;
                    hit.stale = false;
                }
                // The expanded pane anchors on this hit and its blob
                // is still in flight: adopt the located line so the
                // cursor lands right when the file lands (plans/0012
                // M2). Once loaded, the user owns the cursor.
                if let Some(exp) = &mut self.expanded
                    && !exp.loaded
                    && exp.hit.repo == *repo
                    && exp.hit.path == *path
                    && exp.hit.sha == *sha
                {
                    exp.hit.line = *line;
                }
            }
            // Expanded pane (plans/0012 M2): the whole blob landed,
            // already sanitized + highlighted by App. Identity match
            // drops fetches a later expand superseded.
            Action::HitFileLoaded {
                repo,
                path,
                sha,
                lang,
                lines,
            } => {
                if let Some(exp) = &mut self.expanded
                    && exp.hit.repo == *repo
                    && exp.hit.path == *path
                    && exp.hit.sha == *sha
                {
                    let anchor = exp.hit.line;
                    exp.preview.set_highlighted(path, lang, lines.clone());
                    exp.preview.title = file_title(&exp.hit);
                    exp.preview.set_cursor_line(anchor);
                    exp.loaded = true;
                }
            }
            Action::HitFileFailed { error, .. } => {
                if let Some(exp) = &mut self.expanded {
                    // Same surface as the browser's failed blob: the
                    // pane itself says what went wrong; Esc still
                    // folds back to the results.
                    exp.preview.content = PreviewContent::Text(format!(
                        "error: {}",
                        crate::app::provider_status(error)
                    ));
                    exp.loaded = true;
                }
            }
            // v1.2 (plans/0008 §4): the blob answered but the match
            // text isn't in it — flip to unlocatable (never
            // self-heals) instead of rendering stale forever.
            Action::HitContextMissing { sha } => {
                for hit in self.hits.iter_mut().filter(|h| &h.sha == sha) {
                    hit.stale = false;
                    hit.unlocatable = true;
                }
            }
            _ => {}
        }
    }

    /// Streamed batch (v1.3, plans/0011): merge same-file hits into
    /// their block, append the rest. Past RENDER_CAP hits are counted
    /// (`dropped`) and skipped.
    pub fn append_hits(&mut self, hits: Vec<SearchHit>) {
        for hit in hits {
            if self.hits.len() >= RENDER_CAP {
                self.dropped += 1;
                continue;
            }
            if let Some(existing) = self
                .hits
                .iter_mut()
                .find(|h| h.repo == hit.repo && h.path == hit.path)
            {
                existing.merge(hit);
            } else {
                self.hits.push(hit);
            }
        }
        self.clamp_selection();
    }

    /// Hits kept so far (streamed or replaced) — the modeline's live
    /// count while a search streams.
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }
}
