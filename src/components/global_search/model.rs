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

/// Grep: restyle occurrences of `query` inside preview lines with the
/// theme's match chip (search_match bg, crust fg). Stage 2 will use the
/// API's text-match ranges instead of re-finding the substring.
/// Byte offsets come from the lowercased text — exact for ASCII,
/// cosmetic-only drift on exotic unicode case folds.
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
    let chip = ratatui::style::Style::default()
        .fg(match_fg)
        .bg(match_bg)
        .add_modifier(Modifier::BOLD);
    for hit in hits {
        for (_, line) in &mut hit.preview {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for span in &line.spans {
                let text = span.content.to_string();
                let lower = text.to_lowercase();
                let mut at = 0; // byte offset into `text`
                let mut rest = lower.as_str();
                while let Some(pos) = rest.find(&needle) {
                    let (start, end) = (at + pos, at + pos + needle.len());
                    if start > at {
                        spans.push(Span::styled(text[at..start].to_string(), span.style));
                    }
                    spans.push(Span::styled(text[start..end].to_string(), chip));
                    at = end;
                    rest = &lower[end..];
                }
                if at < text.len() {
                    spans.push(Span::styled(text[at..].to_string(), span.style));
                }
            }
            *line = Line::from(spans);
        }
    }
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
}
