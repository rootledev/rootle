//! Read-only commit composition: shared list mechanics, a persistent files
//! sidebar, and the shared preview for prose. Patches are prepared on actions.

mod files;
mod prepare;
mod render;
mod rows;
mod search;
mod yank;
pub(crate) use yank::CommitYankTarget;
#[cfg(test)]
mod tests;

use crate::action::Action;
use crate::components::Component;
use crate::components::list_view::{
    Boundary, FilterOutcome, ItemIndex, ListCursor, ListFilter, ListMovement, Viewport,
};
use crate::components::preview::Preview;
use crate::highlight::Highlighter;
use crate::request::CommitRequest;
use crate::theme::Theme;
use prepare::CommitContent;
use ratatui::{Frame, layout::Rect};
use rootle_provider::CommitDetail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitFocus {
    Files,
    Preview,
}

enum CommitLoad {
    Loading,
    Failed(String),
    Ready(Box<CommitContent>),
}

struct OpenDelta {
    file: ItemIndex,
    selection: ListCursor,
    viewport: Viewport,
    horizontal: usize,
    text_width: usize,
    search: search::DiffSearch,
}

pub struct CommitView {
    request: CommitRequest,
    load: CommitLoad,
    selection: ListCursor,
    viewport: Viewport,
    message: Preview,
    highlighter: Highlighter,
    filter: ListFilter,
    filter_selection: Option<ItemIndex>,
    filter_preview_open: bool,
    focus: CommitFocus,
    delta: Option<OpenDelta>,
    keys: crate::keymap::KeySequence,
}

impl CommitView {
    pub fn open(request: CommitRequest) -> Self {
        Self {
            request,
            load: CommitLoad::Loading,
            selection: ListCursor::new(),
            viewport: Viewport::default(),
            message: Preview::new(),
            highlighter: Highlighter::default(),
            filter: ListFilter::default(),
            filter_selection: None,
            filter_preview_open: false,
            focus: CommitFocus::Files,
            delta: None,
            keys: crate::keymap::KeySequence::default(),
        }
    }

    pub fn request(&self) -> &CommitRequest {
        &self.request
    }

    pub fn loaded(&mut self, request: &CommitRequest, detail: CommitDetail, theme: &Theme) {
        if request != &self.request {
            return;
        }
        if detail.sha != request.revision {
            self.load = CommitLoad::Failed("provider returned a different commit".into());
            return;
        }
        self.selection.reset();
        self.viewport.reset();
        self.filter = ListFilter::default();
        self.focus = CommitFocus::Files;
        self.delta = None;
        self.highlighter.set_theme(theme);
        let mut content = CommitContent::new(detail);
        self.message
            .set_text("commit message", std::mem::take(&mut content.message));
        self.load = CommitLoad::Ready(Box::new(content));
    }

    pub fn set_theme(&mut self, theme: &Theme) {
        self.highlighter.set_theme(theme);
        if let CommitLoad::Ready(content) = &mut self.load {
            content.restyle(&self.highlighter);
        }
    }

    pub fn failed(&mut self, request: &CommitRequest, error: String) {
        if request == &self.request {
            self.load = CommitLoad::Failed(crate::sanitize::sanitize_inline(&error));
        }
    }

    pub fn move_cursor(&mut self, movement: ListMovement) {
        let CommitLoad::Ready(content) = &self.load else {
            return;
        };
        if self.focus == CommitFocus::Preview {
            if let Some(delta) = &mut self.delta {
                let count = content.files[delta.file.get()]
                    .prepared
                    .as_ref()
                    .map_or(0, |patch| patch.rows.len());
                delta.selection.advance(movement, count, Boundary::Clamp);
            } else {
                self.message.scroll_text(movement);
            }
        } else {
            self.selection
                .advance(movement, self.visible_files(content).len(), Boundary::Clamp);
            if self.delta.is_some() {
                self.refresh_delta();
            }
        }
    }

    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            CommitFocus::Files => CommitFocus::Preview,
            CommitFocus::Preview => CommitFocus::Files,
        };
    }

    pub fn horizontal(&mut self, forward: bool) {
        if self.focus == CommitFocus::Preview
            && let Some(delta) = &mut self.delta
        {
            delta.horizontal = if forward {
                delta.horizontal.saturating_add(1)
            } else {
                delta.horizontal.saturating_sub(1)
            };
        }
    }

    pub fn open_delta(&mut self) {
        if let Some(file) = self.selected_file() {
            self.open_file_at(file);
            self.focus = CommitFocus::Preview;
        }
    }

    pub fn step_file(&mut self, movement: ListMovement) {
        let CommitLoad::Ready(content) = &self.load else {
            return;
        };
        let visible = self.visible_files(content);
        if visible.is_empty() {
            return;
        }
        self.selection
            .advance(movement, visible.len(), Boundary::Wrap);
        self.open_file_at(visible[self.selection.selected().get()]);
    }

    fn selected_file(&self) -> Option<ItemIndex> {
        let CommitLoad::Ready(content) = &self.load else {
            return None;
        };
        self.visible_files(content)
            .get(self.selection.selected().get())
            .copied()
    }

    fn refresh_delta(&mut self) {
        if let Some(file) = self.selected_file() {
            self.open_file_at(file);
        } else {
            self.delta = None;
        }
    }

    fn open_file_at(&mut self, file: ItemIndex) {
        if self.delta.as_ref().is_some_and(|delta| delta.file == file) {
            return;
        }
        let CommitLoad::Ready(content) = &mut self.load else {
            return;
        };
        content.prepare(file, &self.highlighter);
        self.delta = Some(OpenDelta {
            file,
            selection: ListCursor::new(),
            viewport: Viewport::default(),
            horizontal: 0,
            text_width: 0,
            search: search::DiffSearch::default(),
        });
    }

    /// A provider-supplied commit permalink, never synthesized as a repo URL.
    pub fn web_url(&self) -> Option<&str> {
        let CommitLoad::Ready(content) = &self.load else {
            return None;
        };
        content
            .detail
            .web_url
            .as_deref()
            .filter(|url| !url.chars().any(char::is_control))
    }

    pub fn escape(&mut self) -> bool {
        let selected = self.selected_file();
        if self.focus == CommitFocus::Preview
            && let Some(delta) = &mut self.delta
            && delta.search.clear()
        {
            return false;
        }
        if self.focus == CommitFocus::Files && self.filter.clear() {
            self.restore_selection(selected);
            return false;
        }
        if self.delta.take().is_some() {
            self.focus = CommitFocus::Files;
            return false;
        }
        true
    }

    pub fn filtering(&self) -> bool {
        self.filter.active()
    }

    pub fn begin_filter(&mut self) {
        self.filter_selection = self.selected_file();
        self.filter_preview_open = self.delta.is_some();
        self.focus = CommitFocus::Files;
        self.filter.begin();
    }

    pub fn filter_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        let selected = self.selected_file();
        let outcome = self.filter.handle_key(key);
        if outcome != FilterOutcome::Unchanged {
            let preferred = if outcome == FilterOutcome::Restored {
                self.filter_selection.take()
            } else {
                selected
            };
            self.restore_selection(preferred);
            if self.filter_preview_open && self.delta.is_none() {
                self.refresh_delta();
            }
            if matches!(outcome, FilterOutcome::Restored | FilterOutcome::Committed) {
                self.filter_preview_open = false;
            }
        }
    }

    fn restore_selection(&mut self, preferred: Option<ItemIndex>) {
        let CommitLoad::Ready(content) = &self.load else {
            return;
        };
        let visible = self.visible_files(content);
        let position = preferred
            .and_then(|selected| visible.iter().position(|file| *file == selected))
            .unwrap_or(0);
        self.selection.select(ItemIndex::new(position));
        self.viewport.reset();
        if self.delta.is_some() {
            self.refresh_delta();
        }
    }

    fn visible_files(&self, content: &CommitContent) -> Vec<ItemIndex> {
        content
            .tree
            .file_order()
            .iter()
            .copied()
            .filter(|index| {
                let file = &content.files[index.get()];
                self.filter.matches(&file.label)
                    || self.filter.matches(file.status.label())
                    || file
                        .previous_label
                        .as_ref()
                        .is_some_and(|path| self.filter.matches(path))
            })
            .collect()
    }

    fn input_context(&self) -> crate::keymap::CommitContext {
        match self.focus {
            CommitFocus::Files => crate::keymap::CommitContext::Files,
            CommitFocus::Preview if self.delta.is_some() => crate::keymap::CommitContext::Diff,
            CommitFocus::Preview => crate::keymap::CommitContext::Message,
        }
    }

    pub(crate) fn hints(&self) -> &'static [crate::keymap::Hint] {
        crate::keymap::commit_hints(self.input_context())
    }

    pub fn open_file(&self) -> Option<ItemIndex> {
        self.delta.as_ref().map(|delta| delta.file)
    }
    pub fn delta_open(&self) -> bool {
        self.delta.is_some()
    }
    pub fn sha_short(&self) -> String {
        crate::sanitize::sanitize_inline(&self.request.revision.short())
    }
    pub fn file_count(&self) -> usize {
        match &self.load {
            CommitLoad::Ready(content) => content.files.len(),
            _ => 0,
        }
    }
}

impl Component for CommitView {
    fn handle_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> Action {
        if self.filter.active() {
            return Action::CommitFilterKey(key);
        }
        if self.searching() {
            return Action::CommitSearchKey(key);
        }
        let context = self.input_context();
        crate::keymap::commit(key, &mut self.keys, context)
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::CommitUp => self.move_cursor(ListMovement::Previous),
            Action::CommitDown => self.move_cursor(ListMovement::Next),
            Action::CommitFirst => self.move_cursor(ListMovement::First),
            Action::CommitLast => self.move_cursor(ListMovement::Last),
            Action::CommitOpen => self.open_delta(),
            Action::CommitStepNext => self.step_file(ListMovement::Next),
            Action::CommitStepPrev => self.step_file(ListMovement::Previous),
            Action::CommitFocus => self.focus_next(),
            Action::CommitLeft => self.horizontal(false),
            Action::CommitRight => self.horizontal(true),
            Action::CommitSearchBegin => {
                self.keys.clear();
                self.begin_diff_search();
            }
            Action::CommitSearchKey(key) => self.diff_search_key(*key),
            Action::CommitSearchNext => self.step_diff_match(true),
            Action::CommitSearchPrevious => self.step_diff_match(false),
            Action::CommitPage { forward, half } => self.page(*forward, *half),
            Action::CommitFilterBegin => {
                self.keys.clear();
                self.begin_filter();
            }
            Action::CommitFilterKey(key) => self.filter_key(*key),
            _ => {}
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        render::draw(self, frame, area, theme);
    }
}
