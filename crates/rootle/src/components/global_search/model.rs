//! Search data model: kinds, scopes, and the hit shape shared by the
//! view (UI thread), the backend (worker threads), and the mock
//! producer. Nothing here draws or owns keys.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    FileFind,
    Grep,
}

impl SearchKind {
    pub fn title(self) -> &'static str {
        match self {
            SearchKind::FileFind => " find file ",
            SearchKind::Grep => " grep ",
        }
    }

    /// Cache-edit slug for materialized mock files.
    pub fn slug(self) -> &'static str {
        match self {
            SearchKind::FileFind => "find",
            SearchKind::Grep => "grep",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The repo currently open in the browser.
    Repo,
    /// The org selected in the browser's top level.
    Org,
    /// All of GitHub.
    Global,
}

impl Scope {
    /// Persisted form in state.json.
    pub fn stored(self) -> &'static str {
        match self {
            Scope::Repo => "repo",
            Scope::Org => "org",
            Scope::Global => "global",
        }
    }

    pub fn from_stored(s: &str) -> Option<Scope> {
        match s {
            "repo" => Some(Scope::Repo),
            "org" => Some(Scope::Org),
            "global" => Some(Scope::Global),
            _ => None,
        }
    }
}

/// Raw backend result from a worker thread — converted to a
/// `SearchHit` on the UI thread (highlight boundary, like blobs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHit {
    pub repo: String,
    pub path: String,
    pub sha: String,
    pub branch: String,
    pub line: u32,
    pub preview: Vec<(u32, String)>,
    pub match_count: u32,
    /// v1.1 `located: false` — provider knows its index is stale for
    /// this hit; cleared once client-side locating succeeds.
    pub stale: bool,
}

/// One result: full path + highlighted preview lines (`line_no`, `Line`).
/// Multiple matches in one file fold into a single block; `match_count`
/// carries the badge shown next to the path (0 = path match / unknown).
/// `body` is the materializable file content for the editor when no
/// blob sha is known (mock); real hits open via `sha` + `repo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Owning repo ("owner/name"); mock fills a stand-in, the stage-2
    /// backend fills the real one. Drives yank URLs and blob fetches.
    pub repo: String,
    pub path: String,
    /// Blob sha (empty for mock hits) + the repo's default branch.
    pub sha: String,
    pub branch: String,
    pub line: u32,
    pub preview: Vec<(u32, Line<'static>)>,
    pub match_count: u32,
    pub body: String,
    /// v1.1: provider flagged the placement stale (`located: false`).
    pub stale: bool,
    /// v1.2 (plans/0008 §4): the blob was fetched but the match text
    /// isn't in it — non-literal matches (stemmed/semantic) or moved
    /// content. Distinct from `stale`: this never self-heals.
    pub unlocatable: bool,
}

impl SearchHit {
    /// Unhighlighted hit (mock producer, tests): plain-text Lines.
    pub fn plain(
        repo: &str,
        path: &str,
        line: u32,
        preview: Vec<(u32, String)>,
        match_count: u32,
        body: String,
    ) -> Self {
        SearchHit {
            repo: repo.into(),
            path: path.into(),
            sha: String::new(),
            branch: "main".into(),
            line,
            preview: preview
                .into_iter()
                .map(|(no, text)| (no, Line::from(Span::raw(text))))
                .collect(),
            match_count,
            body,
            stale: false,
            unlocatable: false,
        }
    }

    /// A raw backend hit, still unhighlighted (UI thread styles it).
    pub fn from_raw(raw: RawHit) -> Self {
        let mut hit = SearchHit::plain(
            &raw.repo,
            &raw.path,
            raw.line,
            raw.preview,
            raw.match_count,
            String::new(),
        );
        hit.sha = raw.sha;
        hit.branch = raw.branch;
        hit.stale = raw.stale;
        hit
    }

    /// Fold a streamed batch hit (v1.3, plans/0011) into this one:
    /// same file — union the preview regions (sorted, deduped by line),
    /// sum the badges, let a located batch heal a stale placement.
    /// Last write wins for sha/branch: the later batch is the likelier
    /// truth under index drift.
    pub fn merge(&mut self, later: SearchHit) {
        self.sha = later.sha;
        self.branch = later.branch;
        if later.line > 0 && (self.line == 0 || later.line < self.line) {
            self.line = later.line;
        }
        self.match_count = self.match_count.saturating_add(later.match_count);
        self.preview.extend(later.preview);
        self.preview.sort_by_key(|(no, _)| *no);
        self.preview.dedup_by_key(|(no, _)| *no);
        self.stale &= later.stale;
        self.unlocatable &= later.unlocatable;
        if later.body.len() > self.body.len() {
            self.body = later.body;
        }
    }

    /// Preview text, one line per entry — the highlighter's input.
    pub fn preview_text(&self) -> String {
        self.preview
            .iter()
            .map(|(_, line)| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Replace the preview styling while keeping line numbers.
    pub fn with_highlighted(self, lines: Vec<Line<'static>>) -> Self {
        let preview = self
            .preview
            .into_iter()
            .zip(lines)
            .map(|((no, _), line)| (no, line))
            .collect();
        SearchHit { preview, ..self }
    }
}

/// Plain-text content of a rendered line (for `/` filtering).
pub(super) fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Grep: restyle occurrences of `query` with the theme's match chip
/// (search_match bg, crust fg) — boundary-aware: the needle is found
/// over the WHOLE line, then spans are split at match edges, so a
/// match straddling syntax spans (a word half in a comment) still
/// chips.
pub fn highlight_matches(
    hits: &mut [SearchHit],
    query: &str,
    match_bg: ratatui::style::Color,
    match_fg: ratatui::style::Color,
) {
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return;
    }
    for hit in hits {
        for (_, line) in &mut hit.preview {
            chip_line(line, &needle, match_bg, match_fg);
        }
    }
}

/// One line, one needle: join the spans, find every case-insensitive
/// occurrence, then rebuild the span list splitting at match edges so
/// each surviving piece keeps its own syntax style while matches take
pub fn chip_line(
    line: &mut Line<'static>,
    needle: &str,
    bg: ratatui::style::Color,
    fg: ratatui::style::Color,
) {
    if needle.is_empty() || line.spans.is_empty() {
        return;
    }
    // Case folds that change byte length would misalign the offsets —
    // skip them (cosmetic loss on exotic unicode, never a panic).
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let lower = text.to_lowercase();
    if lower.len() != text.len() {
        return;
    }
    let mut matches: Vec<(usize, usize)> = Vec::new();
    let mut rest = lower.as_str();
    let mut at = 0;
    while let Some(pos) = rest.find(needle) {
        matches.push((at + pos, at + pos + needle.len()));
        at += pos + needle.len();
        rest = &lower[at..];
    }
    if matches.is_empty() {
        return;
    }
    let chip = ratatui::style::Style::default()
        .fg(fg)
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    // Walk the spans, cutting each at the match edges it overlaps —
    // surviving pieces keep their syntax style, matches take the chip.
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut offset = 0usize;
    for span in std::mem::take(&mut line.spans) {
        let content = span.content;
        let len = content.len();
        let (start, end) = (offset, offset + len);
        let mut piece_start = start;
        for (m0, m1) in &matches {
            let (m0, m1) = (*m0, *m1);
            if m1 <= piece_start || m0 >= end {
                continue;
            }
            let cut_lo = m0.max(piece_start);
            if cut_lo > piece_start {
                spans.push(Span::styled(
                    content[piece_start - start..cut_lo - start].to_string(),
                    span.style,
                ));
            }
            let cut_hi = m1.min(end);
            spans.push(Span::styled(
                content[cut_lo - start..cut_hi - start].to_string(),
                chip,
            ));
            piece_start = cut_hi;
        }
        if piece_start < end {
            spans.push(Span::styled(
                content[piece_start - start..].to_string(),
                span.style,
            ));
        }
        offset = end;
    }
    line.spans = spans;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::global_search::mock;
    use ratatui::style::Color;

    #[test]
    fn highlight_matches_chips_query_spans() {
        let mut hits = mock::hits(SearchKind::Grep, "query", "");
        highlight_matches(&mut hits, "query", Color::Yellow, Color::Black);
        let hit = hits.iter().find(|h| h.path == "src/terminal.rs").unwrap();
        // "let query = self.frame_query.take();" — two occurrences.
        let line = &hit.preview[2].1;
        let chipped: Vec<&Span> = line
            .spans
            .iter()
            .filter(|s| s.style.bg == Some(Color::Yellow))
            .collect();
        assert_eq!(chipped.len(), 2);
        assert!(chipped.iter().all(|s| s.content.as_ref() == "query"));
    }

    /// Boundary-aware chipping (0019): a needle straddling syntax
    /// spans still chips, and surviving pieces keep their styles.
    #[test]
    fn chip_line_splits_at_match_edges_across_spans() {
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};

        // "// comm" + "ent struct x" — `struct` sits in the second
        // span; a needle split across spans must chip too.
        let mut line = Line::from(vec![
            Span::styled("// comm".to_string(), Style::default().fg(Color::Green)),
            Span::styled(
                "ent struct x".to_string(),
                Style::default().fg(Color::Green),
            ),
        ]);
        chip_line(&mut line, "struct", Color::Yellow, Color::Black);
        let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "// comment struct x", "text unchanged");
        let chipped: Vec<&Span> = line
            .spans
            .iter()
            .filter(|s| s.style.bg == Some(Color::Yellow))
            .collect();
        assert_eq!(chipped.len(), 1, "one chip: {:?}", line.spans);
        assert_eq!(chipped[0].content, "struct");
        assert!(chipped[0].style.add_modifier.contains(Modifier::BOLD));
        assert!(
            line.spans
                .iter()
                .all(|s| { s.style.bg == Some(Color::Yellow) || s.style.fg == Some(Color::Green) })
        );

        // Straddling: "ent str" crosses the span boundary.
        let mut line = Line::from(vec![
            Span::styled("// comm".to_string(), Style::default().fg(Color::Green)),
            Span::styled(
                "ent struct x".to_string(),
                Style::default().fg(Color::Green),
            ),
        ]);
        chip_line(&mut line, "ent str", Color::Yellow, Color::Black);
        let chipped: Vec<&Span> = line
            .spans
            .iter()
            .filter(|s| s.style.bg == Some(Color::Yellow))
            .collect();
        assert_eq!(
            chipped.len(),
            1,
            "straddling needle still chips: {:?}",
            line.spans
        );
        assert_eq!(chipped[0].content, "ent str");

        // No match: spans untouched.
        let mut line = Line::from(vec![Span::raw("plain".to_string())]);
        chip_line(&mut line, "zzz", Color::Yellow, Color::Black);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "plain");
    }
}
