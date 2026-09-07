use crate::{DiffLine, FileDiff, Hunk, HunkRange, LineNumber, LineOrigin};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchErrorKind {
    Header,
    UnexpectedLine,
    MissingNewlineTarget,
    IncompleteHunk,
    ExcessLines,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchError {
    pub patch_line: usize,
    pub kind: PatchErrorKind,
}
impl std::fmt::Display for PatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self.kind {
            PatchErrorKind::Header => "invalid hunk range",
            PatchErrorKind::UnexpectedLine => "unprefixed or unexpected patch line",
            PatchErrorKind::MissingNewlineTarget => "newline marker without a preceding line",
            PatchErrorKind::IncompleteHunk => "incomplete hunk (patch may be truncated)",
            PatchErrorKind::ExcessLines => "hunk contains more lines than its range",
        };
        write!(formatter, "patch line {}: {reason}", self.patch_line)
    }
}
impl std::error::Error for PatchError {}

impl FileDiff {
    pub fn parse(patch: &str) -> Result<Self, PatchError> {
        let mut diff = Self::default();
        let mut consumed_old = 0;
        let mut consumed_new = 0;
        let mut last_line = 0;
        for (index, line) in patch.split_terminator('\n').enumerate() {
            let patch_line = index + 1;
            last_line = patch_line;
            let error = |kind| PatchError { patch_line, kind };
            if line.starts_with("@@") {
                finish(&diff, consumed_old, consumed_new, patch_line)?;
                let (old, new, context) =
                    parse_header(line).ok_or_else(|| error(PatchErrorKind::Header))?;
                diff.hunks.push(Hunk {
                    old,
                    new,
                    context: context.to_string(),
                    lines: Vec::new(),
                });
                consumed_old = 0;
                consumed_new = 0;
                continue;
            }
            if line == "\\ No newline at end of file" {
                let target = diff
                    .hunks
                    .last_mut()
                    .and_then(|hunk| hunk.lines.last_mut())
                    .ok_or_else(|| error(PatchErrorKind::MissingNewlineTarget))?;
                target.has_newline = false;
                continue;
            }
            let Some(hunk) = diff.hunks.last_mut() else {
                if line.is_empty()
                    || [
                        "diff --git ",
                        "index ",
                        "--- ",
                        "+++ ",
                        "new file mode ",
                        "deleted file mode ",
                        "old mode ",
                        "new mode ",
                        "similarity index ",
                        "rename from ",
                        "rename to ",
                    ]
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
                {
                    continue;
                }
                return Err(error(PatchErrorKind::UnexpectedLine));
            };
            let (origin, text) = if let Some(text) = line.strip_prefix('+') {
                (LineOrigin::Addition, text)
            } else if let Some(text) = line.strip_prefix('-') {
                (LineOrigin::Deletion, text)
            } else if let Some(text) = line.strip_prefix(' ') {
                (LineOrigin::Context, text)
            } else {
                return Err(error(PatchErrorKind::UnexpectedLine));
            };
            let old_line = if origin != LineOrigin::Addition {
                Some(
                    take_line(hunk.old, &mut consumed_old)
                        .ok_or_else(|| error(PatchErrorKind::ExcessLines))?,
                )
            } else {
                None
            };
            let new_line = if origin != LineOrigin::Deletion {
                Some(
                    take_line(hunk.new, &mut consumed_new)
                        .ok_or_else(|| error(PatchErrorKind::ExcessLines))?,
                )
            } else {
                None
            };
            match origin {
                LineOrigin::Addition => diff.added += 1,
                LineOrigin::Deletion => diff.deleted += 1,
                LineOrigin::Context => {}
            }
            // The transport may omit the patch string's final newline.
            // Only the explicit marker describes the source file's EOF.
            hunk.lines.push(DiffLine {
                origin,
                old_line,
                new_line,
                text: text.to_string(),
                has_newline: true,
            });
        }
        finish(&diff, consumed_old, consumed_new, last_line)?;
        Ok(diff)
    }
}

fn take_line(range: HunkRange, consumed: &mut u32) -> Option<LineNumber> {
    if *consumed >= range.count {
        return None;
    }
    let line = LineNumber::new(range.start.checked_add(*consumed)?)?;
    *consumed += 1;
    Some(line)
}

fn finish(diff: &FileDiff, old: u32, new: u32, patch_line: usize) -> Result<(), PatchError> {
    if let Some(hunk) = diff.hunks.last()
        && (old != hunk.old.count || new != hunk.new.count)
    {
        return Err(PatchError {
            patch_line,
            kind: PatchErrorKind::IncompleteHunk,
        });
    }
    Ok(())
}

fn parse_header(line: &str) -> Option<(HunkRange, HunkRange, &str)> {
    let (ranges, context) = line.strip_prefix("@@ ")?.split_once(" @@")?;
    let (old, new) = ranges.split_once(' ')?;
    Some((
        parse_range(old.strip_prefix('-')?)?,
        parse_range(new.strip_prefix('+')?)?,
        context.trim_start(),
    ))
}

fn parse_range(text: &str) -> Option<HunkRange> {
    let (start, count) = match text.split_once(',') {
        Some((start, count)) => (start.parse::<u32>().ok()?, count.parse::<u32>().ok()?),
        None => (text.parse::<u32>().ok()?, 1),
    };
    if count > 0 {
        LineNumber::new(start)?;
        start.checked_add(count - 1)?;
    }
    Some(HunkRange { start, count })
}
