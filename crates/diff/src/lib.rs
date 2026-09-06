//! rootle's typed diff model (plans/0028 M2): providers return
//! unified patch text; this crate turns it into the typed hunks the
//! commit viewer renders from — never by sniffing `+`/`-` in the
//! text at render time. Plus the delta-style intra-line emphasis
//! engine (del/add run pairing, common-affix trim).

/// One diff row's side of the story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOrigin {
    Context,
    Addition,
    Deletion,
}

/// One hunk row. `text` is network text as-is (CRLF bytes stay;
/// rootle sanitizes at its UI boundary — this seam stays lossy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub origin: LineOrigin,
    /// Absent side is `None`, never `0`.
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub text: String,
    /// False on the row a `\ No newline at end of file` marker
    /// annotates (the renderer draws the marker, not a newline).
    pub has_newline: bool,
}

/// One `@@` hunk: both sides' starts and counts plus the typed rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// The `@@ -a,b +c,d @@` header (counts always explicit — the
    /// inverse of `parse_header`).
    pub fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_count, self.new_start, self.new_count
        )
    }
}

/// One file's patch: hunks plus line totals derived from the rows'
/// origins (the wire's header counts are never trusted).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub hunks: Vec<Hunk>,
    pub added: u32,
    pub deleted: u32,
}

impl FileDiff {
    /// Parse the wire `patch` string — a file's unified hunks. File
    /// headers (`diff --git`, `index`, `---`, `+++`), when present,
    /// land before the first `@@` and are skipped; `\ No newline at
    /// end of file` flags the row it follows; CRLF bytes stay in
    /// `text`.
    pub fn parse(patch: &str) -> FileDiff {
        let mut diff = FileDiff::default();
        // Line counters live per hunk — each header resets them.
        let mut old_line = 0;
        let mut new_line = 0;
        for (line, has_newline) in SplitLines(patch) {
            if let Some((old_start, old_count, new_start, new_count)) = parse_header(line) {
                diff.hunks.push(Hunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    lines: Vec::new(),
                });
                old_line = old_start;
                new_line = new_start;
                continue;
            }
            if line.starts_with('\\') {
                // "\ No newline at end of file" annotates the row it
                // follows; it emits no row of its own.
                if let Some(row) = diff.hunks.last_mut().and_then(|h| h.lines.last_mut()) {
                    row.has_newline = false;
                }
                continue;
            }
            if diff.hunks.is_empty() {
                continue; // file headers / junk before the first @@
            }
            let (origin, text) = if line.is_empty() {
                // A context row that lost its space somewhere along
                // the wire.
                (LineOrigin::Context, "")
            } else {
                match line.as_bytes()[0] {
                    b'+' => (LineOrigin::Addition, &line[1..]),
                    b'-' => (LineOrigin::Deletion, &line[1..]),
                    b' ' => (LineOrigin::Context, &line[1..]),
                    _ => continue, // junk inside a hunk body
                }
            };
            let mut row = DiffLine {
                origin,
                old_lineno: None,
                new_lineno: None,
                text: text.to_string(),
                has_newline,
            };
            match origin {
                LineOrigin::Context => {
                    row.old_lineno = Some(old_line);
                    row.new_lineno = Some(new_line);
                    old_line += 1;
                    new_line += 1;
                }
                LineOrigin::Deletion => {
                    row.old_lineno = Some(old_line);
                    old_line += 1;
                    diff.deleted += 1;
                }
                LineOrigin::Addition => {
                    row.new_lineno = Some(new_line);
                    new_line += 1;
                    diff.added += 1;
                }
            }
            if let Some(hunk) = diff.hunks.last_mut() {
                hunk.lines.push(row);
            }
        }
        diff
    }
}

/// Intra-line emphasis (delta-style, ported from strop's render
/// engine): the byte range within this row's text that actually
/// changed, paired against the opposite-side row at the same index
/// in the hunk's del/add run. `None` for context rows and unmatched
/// rows (pure adds/deletes carry no intra-line span).
pub fn emphasis_ranges(hunk: &Hunk, line_idx: usize) -> Option<(usize, usize)> {
    let lines = &hunk.lines;
    let row = lines.get(line_idx)?;
    match row.origin {
        LineOrigin::Context => None,
        LineOrigin::Deletion => {
            // Del-run start and the add-run right after it.
            let mut run_start = line_idx;
            while run_start > 0 && lines[run_start - 1].origin == LineOrigin::Deletion {
                run_start -= 1;
            }
            let mut add_start = line_idx;
            while add_start < lines.len() && lines[add_start].origin == LineOrigin::Deletion {
                add_start += 1;
            }
            let k = line_idx - run_start;
            lines
                .get(add_start + k)
                .filter(|l| l.origin == LineOrigin::Addition)
                .map(|p| changed_range(&row.text, &p.text))
        }
        LineOrigin::Addition => {
            // Add-run start and the del-run right before it.
            let mut run_start = line_idx;
            while run_start > 0 && lines[run_start - 1].origin == LineOrigin::Addition {
                run_start -= 1;
            }
            let mut del_start = run_start;
            while del_start > 0 && lines[del_start - 1].origin == LineOrigin::Deletion {
                del_start -= 1;
            }
            if del_start == run_start {
                return None; // no paired deletions
            }
            let k = line_idx - run_start;
            lines
                .get(del_start + k)
                .filter(|l| l.origin == LineOrigin::Deletion)
                .map(|p| changed_range(&p.text, &row.text))
        }
    }
}

/// The changed middle of `a` vs `b` after trimming the common prefix
/// and suffix (byte offsets into `a`, char-boundary safe by
/// construction — offsets come from char scans).
fn changed_range(a: &str, b: &str) -> (usize, usize) {
    let prefix: usize = a
        .chars()
        .zip(b.chars())
        .take_while(|(x, y)| x == y)
        .map(|(c, _)| c.len_utf8())
        .sum();
    let suffix: usize = a
        .chars()
        .rev()
        .zip(b.chars().rev())
        .take_while(|(x, y)| x == y)
        .map(|(c, _)| c.len_utf8())
        .sum();
    let end = a.len().saturating_sub(suffix).max(prefix);
    (prefix.min(end), end)
}

/// `@@ -a,b +c,d @@ [context]` → both sides' start and count (count
/// defaults to 1 when omitted; trailing function context after the
/// second `@@` is stripped).
fn parse_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let rest = line.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let (old, new) = rest[..end].split_once(' ')?;
    let range = |s: &str| -> Option<(u32, u32)> {
        Some(match s.split_once(',') {
            Some((start, count)) => (start.parse().ok()?, count.parse().ok()?),
            None => (s.parse().ok()?, 1),
        })
    };
    let (old_start, old_count) = range(old.strip_prefix('-')?)?;
    let (new_start, new_count) = range(new.strip_prefix('+')?)?;
    Some((old_start, old_count, new_start, new_count))
}

/// Split on `\n` only, yielding `(line, had_newline)` — unlike
/// `str::lines`, a trailing `\r` stays in the line (CRLF bytes are
/// content).
struct SplitLines<'a>(&'a str);

impl<'a> Iterator for SplitLines<'a> {
    type Item = (&'a str, bool);

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }
        Some(match self.0.find('\n') {
            Some(i) => {
                let (line, rest) = self.0.split_at(i);
                self.0 = &rest[1..];
                (line, true)
            }
            None => {
                let line = self.0;
                self.0 = "";
                (line, false)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root commit's hunk (`-0,0`): every row is an addition — the
    /// old side is absent on all of them, never 0.
    #[test]
    fn root_commit_rows_have_no_old_side() {
        let diff = FileDiff::parse("@@ -0,0 +1,3 @@\n+fn main() {\n+    todo!()\n+}\n");
        assert_eq!(diff.hunks.len(), 1);
        let hunk = &diff.hunks[0];
        assert_eq!(
            hunk.header(),
            "@@ -0,0 +1,3 @@",
            "full-form headers round-trip"
        );
        for (i, row) in hunk.lines.iter().enumerate() {
            assert_eq!(row.origin, LineOrigin::Addition);
            assert_eq!(row.old_lineno, None, "row {i}: absent side is None");
            assert_eq!(row.new_lineno, Some(i as u32 + 1));
        }
        // Pure adds pair with nothing — no intra-line span.
        assert_eq!(emphasis_ranges(hunk, 0), None);
    }

    /// The inverse shape (a file deleted whole): the new side is
    /// absent on every row, and deletions pair with nothing.
    #[test]
    fn all_deletions_rows_have_no_new_side() {
        let diff = FileDiff::parse("@@ -1,2 +0,0 @@\n-old\n-gone\n");
        let hunk = &diff.hunks[0];
        assert_eq!(hunk.lines[0].old_lineno, Some(1));
        assert_eq!(hunk.lines[1].old_lineno, Some(2));
        assert!(hunk.lines.iter().all(|r| r.new_lineno.is_none()));
        assert_eq!(emphasis_ranges(hunk, 0), None);
        assert_eq!(emphasis_ranges(hunk, 1), None);
    }

    /// Del/add runs pair by index and trim their shared affixes —
    /// strop's `emphasis_trims_shared_affixes` case through the
    /// parser: "let x = hone(a);" vs "let x = hone(b, c);" changes
    /// only byte 13..14 on either side.
    #[test]
    fn paired_runs_trim_shared_affixes() {
        let diff = FileDiff::parse(concat!(
            "@@ -1,5 +1,4 @@ fn ctx\n",
            " fn f() {\n",
            "-let x = hone(a);\n",
            "-    gone();\n",
            "+let x = hone(b, c);\n",
            " }\n"
        ));
        let hunk = &diff.hunks[0];
        // First deletion pairs with the lone addition.
        assert_eq!(emphasis_ranges(hunk, 1), Some((13, 14)));
        // Second deletion has no partner — run over by one.
        assert_eq!(emphasis_ranges(hunk, 2), None);
        // The addition sees the same middle from its own side.
        assert_eq!(emphasis_ranges(hunk, 3), Some((13, 14)));
        // Context never emphasizes.
        assert_eq!(emphasis_ranges(hunk, 0), None);
    }

    /// `\ No newline at end of file` flags the row it follows and
    /// emits no row of its own.
    #[test]
    fn no_final_newline_marker_flags_preceding_row() {
        let diff = FileDiff::parse("@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n");
        let hunk = &diff.hunks[0];
        assert_eq!(hunk.lines.len(), 2, "the marker is not a row");
        assert!(!hunk.lines[0].has_newline);
        assert!(hunk.lines[1].has_newline);
    }

    /// CRLF bytes ride inside `text` — only `\n` is a line break.
    #[test]
    fn crlf_bytes_stay_in_text() {
        let diff = FileDiff::parse("@@ -1 +1 @@\n-old\r\n+new\r\n");
        let hunk = &diff.hunks[0];
        assert_eq!(hunk.lines[0].text, "old\r");
        assert_eq!(hunk.lines[1].text, "new\r");
        assert!(hunk.lines.iter().all(|r| r.has_newline));
    }

    /// Counts come from the rows' origins, not the header's claim,
    /// and a count-less header defaults its side's count to 1.
    #[test]
    fn counts_derive_from_origins() {
        // Header claims 9/9; rows say otherwise.
        let diff = FileDiff::parse("@@ -1,9 +1,9 @@\n ctx\n-b\n+c\n-d\n");
        assert_eq!(diff.added, 1);
        assert_eq!(diff.deleted, 2);
        assert_eq!(diff.hunks[0].lines[0].old_lineno, Some(1));
        assert_eq!(diff.hunks[0].lines[0].new_lineno, Some(1));
        // Count-less form: "@@ -5 +6 @@" means one line on a side.
        let one = FileDiff::parse("@@ -5 +6 @@\n ctx\n");
        assert_eq!((one.hunks[0].old_count, one.hunks[0].new_count), (1, 1));
        assert_eq!(one.hunks[0].header(), "@@ -5,1 +6,1 @@");
    }

    /// File headers and junk before the first `@@` are skipped, and
    /// each hunk header resets the line counters.
    #[test]
    fn file_headers_skip_and_hunks_reset_counters() {
        let diff = FileDiff::parse(concat!(
            "diff --git a/lib.rs b/lib.rs\n",
            "index 0a1b..2c3d 100644\n",
            "--- a/lib.rs\n",
            "+++ b/lib.rs\n",
            "@@ -10,2 +10,2 @@\n",
            " keep\n",
            "-old\n",
            "+new\n",
            "@@ -40 +40 @@ fn later\n",
            " more\n"
        ));
        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.hunks[0].lines.len(), 3);
        assert_eq!(diff.hunks[1].lines[0].old_lineno, Some(40));
        assert_eq!(diff.hunks[1].lines[0].new_lineno, Some(40));
        // Trailing function context never rides into the header.
        assert_eq!(diff.hunks[1].header(), "@@ -40,1 +40,1 @@");
    }
}
