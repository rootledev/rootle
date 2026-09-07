//! Network-to-display boundary. Raw identities stay intact; presentation
//! strings are sanitized once and patch parsing never runs while drawing.

use crate::components::list_view::ItemIndex;
use crate::highlight::Highlighter;
use ratatui::text::Line;
use rootle_diff::{ChangedSpan, FileDiff, LineNumber, LineOrigin};
use rootle_provider::{CommitDetail, FileStatus, RepoPath};

const TAB_EXPANSION: &str = "    ";

pub(super) struct CommitContent {
    pub detail: CommitDetail,
    pub author: String,
    pub date: String,
    pub message: String,
    pub files: Vec<FilePresentation>,
}

pub(super) struct FilePresentation {
    path: RepoPath,
    previous_path: Option<RepoPath>,
    pub label: String,
    pub previous_label: Option<String>,
    pub status: FileStatus,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub prepared: Option<PreparedPatch>,
    source_patch: Option<String>,
    binary: bool,
}

pub(super) struct PreparedPatch {
    pub rows: Vec<PatchRow>,
    pub number_width: usize,
}

pub(super) enum PatchRow {
    Hunk(String),
    Content(PreparedLine),
    Note(String),
}

pub(super) struct PreparedLine {
    pub origin: LineOrigin,
    pub old_line: Option<LineNumber>,
    pub new_line: Option<LineNumber>,
    pub syntax: Line<'static>,
    pub changed: Option<ChangedSpan>,
}

impl CommitContent {
    pub fn new(mut detail: CommitDetail) -> Self {
        let files = detail
            .files
            .iter_mut()
            .map(|file| FilePresentation {
                path: file.path.clone(),
                previous_path: file.previous_path.clone(),
                label: crate::sanitize::sanitize_inline(file.path.as_str()),
                previous_label: file
                    .previous_path
                    .as_ref()
                    .map(|path| crate::sanitize::sanitize_inline(path.as_str())),
                status: file.status,
                additions: file.additions,
                deletions: file.deletions,
                source_patch: file.patch.take(),
                binary: file.binary,
                prepared: None,
            })
            .collect();
        let author = crate::sanitize::sanitize_inline(&detail.author);
        let date = crate::sanitize::sanitize_inline(&detail.date);
        let message = display_text(&detail.message);
        Self {
            detail,
            author,
            date,
            message,
            files,
        }
    }

    pub fn prepare(&mut self, index: ItemIndex, highlighter: &Highlighter) {
        let file = &mut self.files[index.get()];
        if file.prepared.is_some() {
            return;
        }
        let patch = match file.source_patch.as_deref() {
            Some(source) => match FileDiff::parse(source) {
                Ok(diff) if diff.hunks.is_empty() => {
                    PreparedPatch::notice("No textual changes (metadata-only change)")
                }
                Ok(diff) => PreparedPatch::from_diff(
                    diff,
                    &file.path,
                    file.previous_path.as_ref(),
                    highlighter,
                ),
                Err(error) => PreparedPatch::notice(&format!("Patch unavailable: {error}")),
            },
            None if file.binary => PreparedPatch::notice("Binary file — no textual patch"),
            None if file.additions == Some(0) && file.deletions == Some(0) => {
                PreparedPatch::notice("No textual changes (metadata-only change)")
            }
            None => PreparedPatch::notice(
                "Patch unavailable — the provider did not supply a textual diff",
            ),
        };
        file.prepared = Some(patch);
    }

    pub fn restyle(&mut self, highlighter: &Highlighter) {
        for index in 0..self.files.len() {
            if self.files[index].prepared.take().is_some() {
                self.prepare(ItemIndex::new(index), highlighter);
            }
        }
    }
}

impl PreparedPatch {
    fn notice(message: &str) -> Self {
        Self {
            rows: vec![PatchRow::Note(message.to_string())],
            number_width: 1,
        }
    }

    fn from_diff(
        mut diff: FileDiff,
        path: &RepoPath,
        previous_path: Option<&RepoPath>,
        highlighter: &Highlighter,
    ) -> Self {
        let number_width = diff
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .flat_map(|line| [line.old_line, line.new_line])
            .flatten()
            .map(|number| number.get())
            .max()
            .unwrap_or(1)
            .to_string()
            .len();
        let mut rows = Vec::new();
        for hunk in &mut diff.hunks {
            rows.push(PatchRow::Hunk(hunk.header()));
            for line in &mut hunk.lines {
                line.text = display_text(&line.text);
            }
            let changes = hunk.changed_spans();
            // Hunks omit intervening source. Parse each side of each hunk
            // separately rather than inventing context across those gaps.
            let side_text = |excluded| {
                let mut source = String::new();
                for line in &hunk.lines {
                    if line.origin != excluded {
                        source.push_str(&line.text);
                        source.push('\n');
                    }
                }
                source
            };
            let old_source = side_text(LineOrigin::Addition);
            let new_source = side_text(LineOrigin::Deletion);
            let mut old_syntax = highlighter
                .highlight(previous_path.unwrap_or(path).as_str(), &old_source)
                .into_iter();
            let mut new_syntax = highlighter
                .highlight(path.as_str(), &new_source)
                .into_iter();
            for (line, changed) in std::mem::take(&mut hunk.lines).into_iter().zip(changes) {
                let has_newline = line.has_newline;
                let syntax = match line.origin {
                    LineOrigin::Deletion => old_syntax.next(),
                    LineOrigin::Addition => new_syntax.next(),
                    LineOrigin::Context => {
                        old_syntax.next();
                        new_syntax.next()
                    }
                }
                .expect("one highlighted line per hunk side row");
                rows.push(PatchRow::Content(PreparedLine {
                    origin: line.origin,
                    old_line: line.old_line,
                    new_line: line.new_line,
                    syntax,
                    changed,
                }));
                if !has_newline {
                    rows.push(PatchRow::Note("\\ No newline at end of file".to_string()));
                }
            }
        }
        Self { rows, number_width }
    }
}

fn display_text(raw: &str) -> String {
    crate::sanitize::sanitize(raw.as_bytes()).replace('\t', TAB_EXPANSION)
}
