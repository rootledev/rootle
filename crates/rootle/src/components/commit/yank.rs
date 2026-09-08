//! Commit and source-line clipboard targets preserve repository/revision/path
//! identity. Deleted rows refer to the first parent and the pre-rename path.
use super::prepare::PatchRow;
use super::{CommitFocus, CommitLoad, CommitView};
use rootle_diff::{LineNumber, LineOrigin};
use rootle_provider::{RepoId, RepoPath, Sha};

pub(crate) enum CommitYankTarget<'a> {
    Commit(&'a str),
    FileLine {
        repository: &'a RepoId,
        path: &'a RepoPath,
        revision: &'a Sha,
        line: LineNumber,
    },
}

impl CommitView {
    pub(crate) fn yank_target(&self) -> Result<CommitYankTarget<'_>, &'static str> {
        let CommitLoad::Ready(content) = &self.load else {
            return Err("commit is not loaded");
        };
        if self.focus == CommitFocus::Preview
            && let Some(delta) = &self.delta
        {
            let file = &content.files[delta.file.get()];
            let prepared = file.prepared.as_ref().expect("opened diff is prepared");
            if let Some(PatchRow::Content(line)) =
                prepared.rows.get(delta.selection.selected().get())
            {
                let (path, revision, number) = match line.origin {
                    LineOrigin::Deletion => (
                        file.previous_path.as_ref().unwrap_or(&file.path),
                        content
                            .detail
                            .parents
                            .first()
                            .ok_or("parent revision unavailable for deleted line")?,
                        line.old_line,
                    ),
                    LineOrigin::Addition | LineOrigin::Context => {
                        (&file.path, &self.request.revision, line.new_line)
                    }
                };
                return Ok(CommitYankTarget::FileLine {
                    repository: &self.request.repository,
                    path,
                    revision,
                    line: number.ok_or("source line number unavailable")?,
                });
            }
        }
        self.web_url()
            .map(CommitYankTarget::Commit)
            .ok_or("provider has no commit permalink")
    }

    pub(crate) fn copy_text(&self) -> Result<String, &'static str> {
        if self.focus != CommitFocus::Preview {
            return Err("focus the preview to copy text");
        }
        let Some(delta) = &self.delta else {
            return self
                .message
                .content_text()
                .ok_or("commit message is not loaded");
        };
        let CommitLoad::Ready(content) = &self.load else {
            return Err("commit is not loaded");
        };
        let prepared = content.files[delta.file.get()]
            .prepared
            .as_ref()
            .expect("opened diff is prepared");
        let Some(PatchRow::Content(line)) = prepared.rows.get(delta.selection.selected().get())
        else {
            return Err("select a source line to copy");
        };
        let mut text = line.syntax.to_string();
        text.push('\n');
        Ok(text)
    }
}
