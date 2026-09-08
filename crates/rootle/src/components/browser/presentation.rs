//! Presentation for Browser.

use super::{
    Action, Browser, Component, Constraint, Direction, EntryKind, Frame, Layout, Rect, Theme,
};

impl Browser {
    pub fn context(&self) -> String {
        // Path up to the focused level (level 0 = orgs, not shown).
        let crumbs = self
            .levels
            .iter()
            .skip(1)
            .take(self.focus)
            .map(|p| p.title.clone())
            .collect::<Vec<_>>()
            .join(" · ");
        // plans/0016 M1a: off-default revisions say so in the crumb.
        match &self.current_ref {
            Some(r) if !crumbs.is_empty() => format!("{crumbs} @ {r}"),
            _ => crumbs,
        }
    }

    /// Preview submode keys (plans/0016 M1, `␣ p`): the vim vertical
    /// motions are owned by `Preview::motion_key` (counts, gg/G,
    /// pages, paragraphs, %, zt/zz/zb); everything else maps through
    /// the named table in keymap.rs (hint rows derive from it).
    pub fn preview_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> Action {
        if self.preview.motion_key(key) {
            return Action::Noop;
        }
        crate::keymap::preview_named(key.code)
    }

    /// The yank anchor: the visual range when one is live, else the
    /// cursor line alone — web_url takes (line, end).
    pub fn yank_anchor(&self) -> (Option<u32>, Option<u32>) {
        match self.preview.visual_range() {
            Some((lo, hi)) => (Some(lo), Some(hi)),
            None => (self.preview.line(), None),
        }
    }

    /// The file under the cursor needs its blob fetched, if any.
    /// Taking a request marks the sha in-flight — without this, the
    /// end-of-route drain would re-request on every keystroke and
    /// recurse (stack overflow, caught by the filter-commit test).
    pub fn take_blob_request(&mut self) -> Option<(String, String)> {
        let (sha, name) = self.blob_request.take()?;
        self.pending_blobs
            .insert(sha.clone())
            .then_some((sha, name))
    }

    /// Re-highlight every cached blob under a new palette and refresh
    /// the visible preview (theme switched, plans/0007 §2).
    pub fn restyle_blobs(&mut self, highlighter: &crate::highlight::Highlighter) {
        for blob in self.blobs.values_mut() {
            blob.lines = highlighter.highlight(&blob.name, &blob.text);
        }
        self.refresh_preview();
    }

    /// Current preview line cursor (1-based) — anchors `␣ y` (v1.1).
    pub fn preview_line(&self) -> Option<u32> {
        self.preview.line()
    }

    pub(super) fn refresh_preview(&mut self) {
        let Some(entry) = self.levels[self.focus].selected_entry().cloned() else {
            self.preview.content = Default::default();
            return;
        };
        match entry.kind {
            EntryKind::File => {
                let base = self.dir_path();
                let full = if base.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{base}/{}", entry.name)
                };
                let node = self.tree.as_ref().and_then(|t| t.find(&full)).cloned();
                match node {
                    Some(node) => {
                        if let Some(blob) = self.blobs.get(&node.sha) {
                            let lines = blob.lines.clone();
                            let lang = blob.lang.clone();
                            self.preview.set_highlighted(&entry.name, &lang, lines);
                        } else if let Some(err) = self.failed_blobs.get(&node.sha) {
                            // Re-selecting a failed fetch re-shows the
                            // honest error — never the loading
                            // placeholder again.
                            let err = err.clone();
                            self.preview
                                .set_error(&entry.name, node.size, &node.sha, &err);
                        } else {
                            if !self.pending_blobs.contains(&node.sha) {
                                self.blob_request = Some((node.sha.clone(), entry.name.clone()));
                            }
                            self.preview
                                .set_file_meta(&entry.name, node.size, &node.sha);
                        }
                    }
                    None => self.preview.content = Default::default(),
                }
            }
            EntryKind::Dir => {
                let children = self.children_of(&entry).unwrap_or_default();
                self.preview.set_dir(&entry.name, children);
            }
            EntryKind::Repo => {
                let children = self.children_of(&entry).unwrap_or_default();
                self.preview.set_dir(&entry.name, children);
            }
            EntryKind::Org | EntryKind::Owner => {
                // Repos load over the API; don't mock them in preview.
                self.preview.title = entry.name.clone();
                self.preview.content = Default::default();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme, zoomed: bool) {
        // ␣ p zoom (tmux `prefix z` model, plans/0016 M1): the preview
        // — or the history lens over it — takes the whole content row;
        // the miller columns are untouched underneath, Esc restores.
        if zoomed {
            if self.commit.is_some() {
                if let Some(view) = self.commit.as_mut() {
                    view.render(frame, area, theme);
                }
            } else if self.history.is_some() {
                self.render_history(frame, area, theme);
            } else {
                self.preview.render(frame, area, theme);
            }
            return;
        }
        // Top level (orgs): fold to a single full-width pane. There is
        // no parent above orgs, so a three-pane split would leave the
        // left column empty (PLAN.md §5).
        if self.focus == 0 {
            self.levels[0].render(frame, area, theme);
            return;
        }

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(40),
                Constraint::Percentage(40),
            ])
            .split(area);

        // Window over the level stack: parent | focused | preview.
        let parent = self.focus - 1;
        self.levels[parent].render(frame, cols[0], theme);
        let focus = self.focus;
        self.levels[focus].render(frame, cols[1], theme);
        // plans/0016 M1b: the history lens swaps the preview's content
        // (same rect, same border idiom) — the preview is untouched
        // underneath and Esc restores it.
        if self.commit.is_some() {
            if let Some(view) = self.commit.as_mut() {
                view.render(frame, cols[2], theme);
            }
        } else if self.history.is_some() {
            self.render_history(frame, cols[2], theme);
        } else {
            self.preview.render(frame, cols[2], theme);
        }
    }
}
