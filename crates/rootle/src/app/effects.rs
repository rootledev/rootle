//! Effects for App.

use super::{Action, App};

impl App {
    /// Fetch the open history lens' commits (v1.5): the previewed
    /// file's log at the browsed revision.
    pub(super) fn open_history_fetch(&mut self) {
        let target = self
            .browser
            .repo_coords()
            .zip(self.browser.history_path().map(str::to_string));
        if let Some(((owner, name), path)) = target {
            let ref_ = self.browser.current_ref().map(str::to_string);
            self.spawn_log(format!("{owner}/{name}"), path, ref_);
        }
    }

    /// Re-highlight cached blobs when the effective theme's syntax
    /// roles change. Cheap no-op per keystroke otherwise (SyntaxSet is
    /// loaded once; only the color table rebuilds).
    pub(super) fn sync_highlight_theme(&mut self) {
        let theme = self.effective_theme();
        if theme.syntax == self.highlight_syntax {
            return;
        }
        self.highlighter.set_theme(&theme);
        self.browser.restyle_blobs(&self.highlighter);
        self.highlight_syntax = theme.syntax;
    }

    /// If the selected file's blob isn't loaded, fetch it.
    pub(super) fn maybe_load_blob(&mut self) {
        if let Some((sha, name)) = self.browser.take_blob_request() {
            self.handle_action(Action::LoadBlob { sha, name });
        }
    }

    /// Hand the prepared editor job to the main loop (which owns the
    /// terminal and performs the suspend/resume dance).
    pub fn take_editor_job(&mut self) -> Option<crate::editor::EditorJob> {
        self.pending_editor.take()
    }

    /// Queued yank text, drained by the main loop once per iteration.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.pending_clipboard.take()
    }

    /// 0019 polish: the band's last-commit context for the file under
    /// preview — memo hit dresses immediately, a miss spawns the
    /// one-shot fetch (ambient; errors stay silent).
    pub(super) fn band_apply_or_fetch(&mut self) {
        if !self.provider.capabilities().log || self.browser.at_commit_view() {
            return;
        }
        let Some((owner, name)) = self.browser.repo_coords() else {
            return;
        };
        let Some((path, _)) = self.browser.selected_file() else {
            return;
        };
        let ref_ = self
            .browser
            .current_ref()
            .map(str::to_string)
            .unwrap_or_default();
        let key = (format!("{owner}/{name}"), path.clone(), ref_.clone());
        match self.last_commits.get(&key) {
            Some(entry) => {
                self.browser.preview.set_band(
                    Some(path),
                    Some(crate::components::preview::BandContext {
                        sha: entry.sha.clone(),
                        subject: entry.subject.clone(),
                        author: entry.author.clone(),
                        date: entry.date.clone(),
                    }),
                );
            }
            None => {
                self.spawn_last_commit(format!("{owner}/{name}"), path, Some(ref_));
            }
        }
    }

    /// A malformed/unreadable config still starts on defaults — but
    /// the warning is sticky-visible (0022 honesty class, 0023 round
    /// 2): degraded slot when free, the status line always.
    pub fn config_warning(&mut self, warning: Option<String>) {
        if let Some(warning) = warning {
            if self.degraded.is_none() {
                self.degraded = Some(warning.clone());
            }
            self.status = Some(warning);
        }
    }

    /// Clear the transient only when it still is this operation's
    /// loading marker — a background success must never erase a fresh
    /// ERROR from an unrelated in-flight operation (0023 round 3: the
    /// default-org warm-up wiped the direct-arg repo's 404).
    pub(crate) fn clear_loading_status(&mut self, prefixes: &[&str]) {
        let Some(status) = &self.status else {
            return;
        };
        let is_loading = status.ends_with('…') && prefixes.iter().any(|p| status.starts_with(p));
        if is_loading {
            self.status = None;
        }
    }

    /// 0018 M3: the quit-time restart trace — only when this session
    /// knew about an update (the `↑` chip), compare the on-disk
    /// binary once. The caller prints it after terminal restore.
    pub fn update_exit_note(&self) -> Option<String> {
        self.update_tag.as_ref()?;
        crate::selfupdate::disk_newer_note()
    }
}
