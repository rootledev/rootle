//! Read-only commit detail and file deltas. Requests have full identity;
//! content is prepared on load/open actions, never from the draw path.

mod prepare;
mod render;
mod rows;
#[cfg(test)]
mod tests;

use crate::action::Action;
use crate::components::Component;
use crate::components::list_view::{
    Boundary, FilterOutcome, ItemIndex, ListCursor, ListFilter, ListMovement, ScrollMovement,
    Viewport,
};
use crate::request::CommitRequest;
use crate::theme::Theme;
use prepare::CommitContent;
use ratatui::{Frame, layout::Rect};
use rootle_provider::CommitDetail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailFocus {
    Files,
    Message,
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
}

pub struct CommitView {
    request: CommitRequest,
    load: CommitLoad,
    selection: ListCursor,
    viewport: Viewport,
    message_viewport: Viewport,
    filter: ListFilter,
    focus: DetailFocus,
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
            message_viewport: Viewport::default(),
            filter: ListFilter::default(),
            focus: DetailFocus::Files,
            delta: None,
            keys: crate::keymap::KeySequence::default(),
        }
    }

    pub fn request(&self) -> &CommitRequest {
        &self.request
    }

    pub fn loaded(&mut self, request: &CommitRequest, detail: CommitDetail) {
        if request != &self.request {
            return;
        }
        if detail.sha != request.revision {
            self.load = CommitLoad::Failed("provider returned a different commit".into());
            return;
        }
        self.selection.reset();
        self.viewport.reset();
        self.delta = None;
        self.load = CommitLoad::Ready(Box::new(CommitContent::new(detail)));
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
        if let Some(delta) = &mut self.delta {
            let count = content.files[delta.file.get()]
                .prepared
                .as_ref()
                .map_or(0, |patch| patch.rows.len());
            delta.selection.advance(movement, count, Boundary::Clamp);
        } else if self.focus == DetailFocus::Message {
            self.message_viewport.scroll(match movement {
                ListMovement::Next => ScrollMovement::Down,
                ListMovement::Previous => ScrollMovement::Up,
                ListMovement::First => ScrollMovement::Top,
                ListMovement::Last => ScrollMovement::Bottom,
            });
        } else {
            self.selection
                .advance(movement, self.visible_files(content).len(), Boundary::Clamp);
        }
    }

    pub fn focus_next(&mut self) {
        if self.delta.is_none() {
            self.focus = match self.focus {
                DetailFocus::Files => DetailFocus::Message,
                DetailFocus::Message => DetailFocus::Files,
            };
        }
    }

    pub fn horizontal(&mut self, forward: bool) {
        if let Some(delta) = &mut self.delta {
            delta.horizontal = if forward {
                delta.horizontal.saturating_add(1)
            } else {
                delta.horizontal.saturating_sub(1)
            };
        }
    }

    pub fn open_delta(&mut self) {
        if self.delta.is_some() {
            return;
        }
        let CommitLoad::Ready(content) = &self.load else {
            return;
        };
        let Some(&file) = self
            .visible_files(content)
            .get(self.selection.selected().get())
        else {
            return;
        };
        self.open_file_at(ItemIndex::new(file));
    }

    pub fn step_file(&mut self, movement: ListMovement) {
        let CommitLoad::Ready(content) = &self.load else {
            return;
        };
        if content.files.is_empty() {
            return;
        }
        let visible = self.visible_files(content);
        let current = self.delta.as_ref().map(|delta| delta.file).or_else(|| {
            visible
                .get(self.selection.selected().get())
                .copied()
                .map(ItemIndex::new)
        });
        let mut selection = ListCursor::new();
        selection.select(current.unwrap_or_default());
        selection.advance(movement, content.files.len(), Boundary::Wrap);
        self.open_file_at(selection.selected());
    }

    fn open_file_at(&mut self, file: ItemIndex) {
        let CommitLoad::Ready(content) = &mut self.load else {
            return;
        };
        content.prepare(file);
        self.delta = Some(OpenDelta {
            file,
            selection: ListCursor::new(),
            viewport: Viewport::default(),
            horizontal: 0,
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
        if let Some(delta) = self.delta.take() {
            if let CommitLoad::Ready(content) = &self.load {
                let visible = self.visible_files(content);
                if let Some(position) = visible.iter().position(|&index| index == delta.file.get())
                {
                    self.selection.select(ItemIndex::new(position));
                }
            }
            return false;
        }
        if self.filter.clear() {
            self.selection.reset();
            self.viewport.reset();
            return false;
        }
        true
    }

    pub fn filtering(&self) -> bool {
        self.filter.active()
    }
    pub fn begin_filter(&mut self) {
        self.delta = None;
        self.focus = DetailFocus::Files;
        self.filter.begin();
    }
    pub fn filter_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        if self.filter.handle_key(key) != FilterOutcome::Unchanged {
            self.selection.reset();
            self.viewport.reset();
        }
    }

    fn visible_files(&self, content: &CommitContent) -> Vec<usize> {
        self.filter.visible(&content.files, |file, filter| {
            filter.matches(&file.label) || filter.matches(file.status.label())
        })
    }
    pub fn open_file(&self) -> Option<usize> {
        self.delta.as_ref().map(|delta| delta.file.get())
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
        crate::keymap::commit(key, &mut self.keys)
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
