//! The commit viewer (plans/0028): commit detail → file delta — the
//! dive chain over the preview pane. State here; rendering in the
//! sibling `render.rs`; the app drives it through `Action`s like
//! every component. The detail arrives via `AppEvent::CommitLoaded`
//! (identity-checked by sha); file deltas parse lazily from the
//! provider's unified patch text through `rootle_diff`.

use crate::components::list_view::ListCursor;
use crate::components::vim_input::VimInput;
use crate::provider::CommitDetail;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;

mod render;

/// One open file delta inside the viewer.
pub struct DiffSurface {
    /// Index into `CommitDetail::files`.
    pub file: usize,
    /// Parsed hunks (None until first render parse or while loading;
    /// binary files stay None with `binary` set).
    parsed: Option<rootle_diff::FileDiff>,
    binary: bool,
    /// Display-line cursor over the delta's flat rows.
    cursor: usize,
    scroll: u16,
}

impl DiffSurface {
    fn at(file: usize) -> Self {
        DiffSurface {
            file,
            parsed: None,
            binary: false,
            cursor: 0,
            scroll: 0,
        }
    }
}

pub struct CommitView {
    /// The commit being inspected (identity for landing checks).
    pub sha: String,
    detail: Option<CommitDetail>,
    failed: Option<String>,
    loading: bool,
    /// Cursor over the visible (filtered) changed-files rows.
    list: ListCursor,
    /// Transient `/` session over the file rows (house rule).
    filter: VimInput,
    filtering: bool,
    filter_value: String,
    pre_filter: String,
    /// The open file delta, if any (detail list otherwise).
    diff: Option<DiffSurface>,
}

impl CommitView {
    pub fn open(sha: &str) -> Self {
        CommitView {
            sha: sha.to_string(),
            detail: None,
            failed: None,
            loading: true,
            list: ListCursor::new(),
            filter: VimInput::transient(),
            filtering: false,
            filter_value: String::new(),
            pre_filter: String::new(),
            diff: None,
        }
    }

    /// `CommitLoaded` landed for this sha (sanitization already
    /// happened at the event boundary).
    pub fn loaded(&mut self, sha: &str, detail: CommitDetail) {
        if sha != self.sha {
            return;
        }
        self.loading = false;
        self.failed = None;
        self.list.clamp(self.visible_files(&detail).len());
        self.detail = Some(detail);
    }

    pub fn failed(&mut self, sha: &str, error: String) {
        if sha != self.sha {
            return;
        }
        self.loading = false;
        self.failed = Some(error);
    }

    pub fn detail(&self) -> Option<&CommitDetail> {
        self.detail.as_ref()
    }

    /// File indices surviving the committed filter (path + status).
    fn visible_files(&self, detail: &CommitDetail) -> Vec<usize> {
        crate::components::list_view::visible_indices(&detail.files, &self.filter_value, |f| {
            format!("{} {}", f.path, f.status.label())
        })
    }

    /// The picked file, if any.
    pub fn picked_file(&self) -> Option<&crate::provider::CommitFile> {
        let detail = self.detail.as_ref()?;
        let vis = self.visible_files(detail);
        let &i = vis.get(self.list.cursor)?;
        detail.files.get(i)
    }

    /// Absolute index of the picked file (for `]f`/`[f` stepping over
    /// ALL files — stepping is navigation, filtering is narrowing).
    fn picked_index(&self) -> Option<usize> {
        let detail = self.detail.as_ref()?;
        let vis = self.visible_files(detail);
        vis.get(self.list.cursor).copied()
    }

    // -- file-list motions ------------------------------------------------

    pub fn move_selection(&mut self, delta: isize) {
        let Some(detail) = self.detail.as_ref() else {
            return;
        };
        let len = self.visible_files(detail).len();
        self.list.r#move(len, delta);
    }

    /// Enter: open the picked file's delta (parse on demand).
    pub fn open_delta(&mut self) {
        let Some(i) = self.picked_index() else {
            return;
        };
        self.diff = Some(DiffSurface::at(i));
    }

    /// `]f`/`[f`: step to the next/previous file, rewriting the open
    /// delta in place (strop's "label · i/n" pattern).
    pub fn step_file(&mut self, delta: isize) {
        let Some(detail) = self.detail.as_ref() else {
            return;
        };
        let n = detail.files.len();
        if n == 0 {
            return;
        }
        let current = self
            .diff
            .as_ref()
            .map(|d| d.file)
            .or_else(|| self.picked_index())
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(n as isize) as usize;
        self.diff = Some(DiffSurface::at(next));
    }

    /// Esc: unwind one surface — open delta closes to the detail
    /// list, a committed filter clears first, the next Esc is the
    /// caller's (returns true = close the whole viewer).
    pub fn escape(&mut self) -> bool {
        if self.diff.is_some() {
            self.diff = None;
            return false;
        }
        if !self.filter_value.is_empty() {
            self.filter_value.clear();
            self.list.clamp(
                self.detail
                    .as_ref()
                    .map(|d| self.visible_files(d).len())
                    .unwrap_or(0),
            );
            return false;
        }
        true
    }

    // -- the `/` session --------------------------------------------------

    pub fn filtering(&self) -> bool {
        self.filtering
    }

    pub fn begin_filter(&mut self) {
        self.filtering = true;
        self.pre_filter = self.filter_value.clone();
    }

    pub fn filter_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        use crate::components::vim_input::Outcome;
        match self.filter.handle_key(key) {
            Outcome::Submitted => {
                self.filtering = false;
                self.filter_value = self.filter.value().to_string();
                if let Some(detail) = self.detail.as_ref() {
                    self.list.clamp(self.visible_files(detail).len());
                }
            }
            Outcome::Cancelled => {
                self.filtering = false;
                self.filter_value = self.pre_filter.clone();
            }
            _ => {}
        }
    }

    /// The open delta's file index, if a delta is open.
    pub fn open_file(&self) -> Option<usize> {
        self.diff.as_ref().map(|d| d.file)
    }

    /// Is a file delta open (vs. the detail file list)?
    pub fn delta_open(&self) -> bool {
        self.diff.is_some()
    }

    pub fn sha_short(&self) -> &str {
        &self.sha[..7.min(self.sha.len())]
    }

    pub fn file_count(&self) -> usize {
        self.detail.as_ref().map(|d| d.files.len()).unwrap_or(0)
    }

    pub fn move_delta(&mut self, delta: isize) {
        let Some(diff) = self.diff.as_mut() else {
            return;
        };
        let rows = render::delta_row_count(diff);
        let next = diff.cursor as isize + delta;
        diff.cursor = next.clamp(0, rows.saturating_sub(1) as isize).max(0) as usize;
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        render::draw(self, frame, area, theme);
    }
}
