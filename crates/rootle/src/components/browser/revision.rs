//! Revision for Browser.

use super::Browser;

impl Browser {
    /// The browsed revision (plans/0016 M1a), if switched off the
    /// default branch.
    pub fn current_ref(&self) -> Option<&str> {
        self.current_ref.as_deref()
    }

    /// Live-preview or commit a revision switch — the crumb follows;
    /// the tree refetch is the app's (LoadRepoTree reads this).
    /// Switching invalidates the revision lenses: history and blame
    /// were fetched for the previous ref.
    pub fn set_current_ref(&mut self, name: Option<String>) {
        self.current_ref = name;
        self.history = None;
        self.commit = None;
        self.blame = None;
        self.preview.set_blame(None);
        self.at_commit = None;
    }

    pub fn close_history(&mut self) {
        self.history = None;
    }

    pub fn open_commit(&mut self, request: crate::request::CommitRequest) {
        self.commit = Some(crate::components::commit::CommitView::open(request));
    }

    pub fn commit_loaded(
        &mut self,
        request: &crate::request::CommitRequest,
        detail: rootle_provider::CommitDetail,
        theme: &crate::theme::Theme,
    ) {
        if let Some(view) = &mut self.commit {
            view.loaded(request, detail, theme);
        }
    }

    pub fn commit_failed(&mut self, request: &crate::request::CommitRequest, error: String) {
        if let Some(view) = &mut self.commit {
            view.failed(request, error);
        }
    }

    pub fn commit(&mut self) -> Option<&mut crate::components::commit::CommitView> {
        self.commit.as_mut()
    }

    pub fn commit_ref(&self) -> Option<&crate::components::commit::CommitView> {
        self.commit.as_ref()
    }

    pub fn history_is_open(&self) -> bool {
        self.history.is_some()
    }

    pub fn close_commit(&mut self) {
        self.commit = None;
    }

    /// Entering open-at-commit: save the present-day blob's identity
    /// (the tree cursor is on the file the lens serves).
    pub fn note_commit_view(&mut self) {
        self.at_commit = self.selected_file();
        // Present-day blame marks are stale over a commit's content.
        self.preview.set_blame(None);
    }
}
