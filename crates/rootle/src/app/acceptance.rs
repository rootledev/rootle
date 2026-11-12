//! Request lifetime admission precedes payload processing or visible effects.
//! Component-owned tree/history/commit identities retain their own guards;
//! their state adapters use the same extracted kernel.

use super::App;
use crate::event::AppEvent;
use rootle_kernel::request::Reply;

impl App {
    pub(super) fn accept_worker_event(&mut self, event: &AppEvent) -> bool {
        match event {
            AppEvent::SearchResults { gen_id, .. } => {
                self.requests.search.receive(gen_id, Reply::Complete)
            }
            AppEvent::SearchFailed { gen_id, .. } => {
                self.requests.search.receive(gen_id, Reply::Failed)
            }
            AppEvent::GlobalSearchDelta { gen_id, .. } => {
                self.requests.view.receive(gen_id, Reply::Partial)
            }
            AppEvent::GlobalSearchResults { gen_id, .. } => {
                self.requests.view.receive(gen_id, Reply::Complete)
            }
            AppEvent::GlobalSearchFailed { gen_id, .. } => {
                self.requests.view.receive(gen_id, Reply::Failed)
            }
            AppEvent::BlobLoaded { generation, .. } => {
                self.requests.blob.accept(*generation, Reply::Complete)
            }
            AppEvent::BlobFailed { generation, .. } => {
                self.requests.blob.accept(*generation, Reply::Failed)
            }
            AppEvent::RefsLoaded { generation, .. } => {
                self.refs_popup.is_some()
                    && self.requests.references.accept(*generation, Reply::Complete)
            }
            AppEvent::RefsFailed { generation, .. } => {
                self.refs_popup.is_some()
                    && self.requests.references.accept(*generation, Reply::Failed)
            }
            AppEvent::BlameLoaded { generation, .. } => {
                self.requests.blame.accept(*generation, Reply::Complete)
            }
            AppEvent::BlameFailed { generation, .. } => {
                self.requests.blame.accept(*generation, Reply::Failed)
            }
            AppEvent::BlobAtLoaded { generation, .. } => {
                self.requests.historical_blob.accept(*generation, Reply::Complete)
            }
            AppEvent::BlobAtFailed { generation, .. } => {
                self.requests.historical_blob.accept(*generation, Reply::Failed)
            }
            AppEvent::LastCommitLoaded { generation, .. } => {
                self.requests.last_commit.accept(*generation, Reply::Complete)
            }
            AppEvent::HitContextLoaded { generation, .. }
            | AppEvent::HitContextMissing { generation, .. } => {
                self.requests.hit_context.accept(*generation, Reply::Complete)
            }
            AppEvent::HitContextFailed { generation, .. } => {
                self.requests.hit_context.accept(*generation, Reply::Failed)
            }
            AppEvent::HitFileLoaded { generation, .. } => {
                self.requests.hit_file.accept(*generation, Reply::Complete)
            }
            AppEvent::HitFileFailed { generation, .. } => {
                self.requests.hit_file.accept(*generation, Reply::Failed)
            }
            // These component-owned lifetimes are checked by the payload handlers.
            AppEvent::OrgReposLoaded { .. }
            | AppEvent::OrgReposFailed { .. }
            | AppEvent::TreeLoaded { .. }
            | AppEvent::TreeFailed { .. }
            | AppEvent::LogDelta { .. }
            | AppEvent::LogLoaded { .. }
            | AppEvent::LogFailed { .. }
            | AppEvent::CommitLoaded { .. }
            // Timer completion is admitted at its Action boundary, also used by tests.
            | AppEvent::HitContextDebounceFired { .. }
            // Process-wide effects, not replaceable data requests.
            | AppEvent::CloneDone { .. }
            | AppEvent::CloneExpanded { .. }
            | AppEvent::UpdateAvailable { .. }
            | AppEvent::DeclarationInstalled { .. }
            | AppEvent::DeclarationFailed { .. } => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{UiEvent, channel};

    #[test]
    fn worker_boundary_drops_superseded_and_duplicate_failures_before_effects() {
        let (sender, _) = channel();
        let mut app = App::with(crate::state::State::default(), sender);
        let old = app.requests.blame.issue();
        let current = app.requests.blame.issue();
        app.status = Some("unchanged".into());
        app.handle_event(UiEvent::Worker(AppEvent::BlameFailed {
            generation: old,
            path: "same/path".into(),
            error: "superseded".into(),
        }));
        assert_eq!(app.status.as_deref(), Some("unchanged"));
        app.handle_event(UiEvent::Worker(AppEvent::BlameFailed {
            generation: current,
            path: "same/path".into(),
            error: "current failure".into(),
        }));
        assert_eq!(app.status.as_deref(), Some("current failure"));
        app.handle_event(UiEvent::Worker(AppEvent::BlameFailed {
            generation: current,
            path: "same/path".into(),
            error: "duplicate failure".into(),
        }));
        assert_eq!(app.status.as_deref(), Some("current failure"));
    }

    #[test]
    fn provider_switch_revokes_worker_tokens_without_reusing_them() {
        let (sender, _) = channel();
        let mut app = App::with(crate::state::State::default(), sender);
        let old = app.requests.blob.issue();
        app.invalidate_provider_context();
        let current = app.requests.blob.issue();
        assert!(current.value() > old.value());
        assert!(!app.accept_worker_event(&AppEvent::BlobLoaded {
            generation: old,
            sha: "same-sha".into(),
            name: "same-path".into(),
            bytes: vec![],
        }));
        assert!(app.accept_worker_event(&AppEvent::BlobLoaded {
            generation: current,
            sha: "same-sha".into(),
            name: "same-path".into(),
            bytes: vec![],
        }));
    }

    #[test]
    fn closing_search_revokes_queued_debounce_before_it_can_spawn_work() {
        let (sender, _) = channel();
        let mut app = App::with(crate::state::State::default(), sender);
        let timer = app.requests.debounce.issue();
        app.handle_action(crate::action::Action::CloseSearchView);
        assert!(!app.requests.debounce.accept(timer, Reply::Complete));
        assert_eq!(app.outstanding_workers().count(), 0);
    }
}
