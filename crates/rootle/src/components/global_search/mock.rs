//! Mock producer (plans/0002 §5): tests and the offline app inject
//! these; the real backend (`backend.rs`) feeds the same `SearchHit`
//! shape.

use super::backend::file_find_score;
use super::model::{SearchHit, SearchKind};

/// (path, first-match line, preview lines as (line_no, text)).
type MockFile = (&'static str, u32, &'static [(u32, &'static str)]);

/// Mock hits honoring the UI inputs the way the stage-2 backend
/// will: file find matches the query against paths, grep against
/// paths + matched lines, and `extension` filters by suffix.
pub fn hits(kind: SearchKind, query: &str, extension: &str) -> Vec<SearchHit> {
    let bodies: &[MockFile] = match kind {
        SearchKind::Grep => &[
            (
                "src/widgets/list.rs",
                42,
                &[
                    (
                        40,
                        "pub fn render(mut self, area: Rect, buf: &mut Buffer) {",
                    ),
                    (41, "    let items = self.items.into_iter()"),
                    (42, "        .filter(|item| item.matches(query))"),
                    (43, "        .collect::<Vec<_>>();"),
                    (44, "    self.render_items(items, area, buf);"),
                    // Second region in the same file — folds into
                    // this block behind an ellipsis separator.
                    (88, "fn rerank(hits: &mut [Hit], query: &str) {"),
                    (89, "    hits.sort_by_key(|hit| hit.score(query));"),
                ],
            ),
            (
                "src/terminal.rs",
                137,
                &[
                    (135, "    /// Flush the diff to the terminal."),
                    (136, "    pub fn flush(&mut self) -> io::Result<()> {"),
                    (137, "        let query = self.frame_query.take();"),
                    (138, "        self.backend.draw(query.iter())?;"),
                    (139, "        Ok(())"),
                ],
            ),
            (
                "src/components/global_search.rs",
                12,
                &[
                    (10, "//! Global search view: fields on top,"),
                    (11, "//! Zed-style result blocks below."),
                    (12, "pub fn query(&self) -> &str {"),
                    (13, "    self.query.value()"),
                    (14, "}"),
                ],
            ),
            (
                "docs/keymap.md",
                3,
                &[
                    (1, "# Keymap"),
                    (2, ""),
                    (3, "Every query starts from the leader key."),
                    (4, "Tab cycles the field row."),
                ],
            ),
        ],
        SearchKind::FileFind => &[
            (
                "src/query/parser.rs",
                1,
                &[
                    (1, "use crate::query::ast::Expr;"),
                    (2, ""),
                    (3, "pub fn parse(input: &str) -> Result<Expr, Error> {"),
                    (4, "    Parser::new(input).expr()"),
                ],
            ),
            (
                "src/query/ast.rs",
                1,
                &[
                    (1, "pub enum Expr {"),
                    (2, "    Term(String),"),
                    (3, "    And(Box<Expr>, Box<Expr>),"),
                    (4, "    Or(Box<Expr>, Box<Expr>),"),
                ],
            ),
            (
                "tests/query_roundtrip.rs",
                1,
                &[
                    (1, "#[test]"),
                    (2, "fn query_roundtrip() {"),
                    (3, "    let q = \"repo:ratatui ext:rs\";"),
                    (4, "    assert_eq!(parse(q).to_string(), q);"),
                ],
            ),
        ],
    };
    let needle = query.to_lowercase();
    let ext = extension.trim_start_matches('.').to_lowercase();
    // File find uses the backend's GitHub-style matcher (so the
    // mock exercises the real semantics); grep stays substring.
    let needles: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut bodies: Vec<&MockFile> = bodies
        .iter()
        .filter(|(path, _, preview)| {
            let path = path.to_lowercase();
            let matches_query = needle.is_empty()
                || match kind {
                    SearchKind::FileFind => file_find_score(&path, &needles).is_some(),
                    SearchKind::Grep => {
                        path.contains(&needle)
                            || preview
                                .iter()
                                .any(|(_, text)| text.to_lowercase().contains(&needle))
                    }
                };
            let matches_ext = ext.is_empty() || path.ends_with(&format!(".{ext}"));
            matches_query && matches_ext
        })
        .collect();
    if kind == SearchKind::FileFind {
        // Same ranking as tree_file_find: best match first.
        bodies.sort_by_key(|(path, _, _)| {
            std::cmp::Reverse(file_find_score(&path.to_lowercase(), &needles))
        });
    }
    bodies
        .iter()
        .map(|(path, line, preview)| {
            let match_count = match kind {
                // Occurrences of the query across the preview lines.
                SearchKind::Grep if !needle.is_empty() => preview
                    .iter()
                    .map(|(_, text)| text.to_lowercase().matches(&needle).count() as u32)
                    .sum(),
                _ => 0, // file find: path match, no content badge
            };
            SearchHit::plain(
                "ratatui/ratatui", // mock repo; stage 2 fills the real one
                path,
                *line,
                preview
                    .iter()
                    .map(|(no, text)| (*no, text.to_string()))
                    .collect(),
                match_count,
                mock_body(path, preview, query),
            )
        })
        .collect()
}

/// Full-file stand-in: the preview lines plus surrounding filler so
/// the editor opens on something that looks real.
fn mock_body(path: &str, preview: &[(u32, &str)], query: &str) -> String {
    let mut body = format!("// mock content for {path} (stage 1, query: {query:?})\n");
    for (_, text) in preview {
        body.push_str(text);
        body.push('\n');
    }
    body.push_str("// …stage 2 replaces this with the fetched blob.\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_match_files_fold_with_count_badge() {
        let hits = super::hits(SearchKind::Grep, "query", "");
        let list = hits
            .iter()
            .find(|h| h.path == "src/widgets/list.rs")
            .unwrap();
        assert_eq!(list.match_count, 3);
        // Both regions live in the one folded block.
        let nos: Vec<u32> = list.preview.iter().map(|(n, _)| *n).collect();
        assert!(nos.contains(&42));
        assert!(nos.contains(&88));
    }

    #[test]
    fn mock_honors_query_and_extension() {
        // File find matches the query against paths.
        let hits = super::hits(SearchKind::FileFind, "parser", "");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/query/parser.rs");
        // Empty query returns everything.
        assert_eq!(super::hits(SearchKind::FileFind, "", "").len(), 3);
        // Grep matches paths and matched lines.
        assert_eq!(super::hits(SearchKind::Grep, "query", "").len(), 4);
        assert_eq!(super::hits(SearchKind::Grep, "flush", "").len(), 1);
        // Extension narrows by suffix (dot optional).
        let rs = super::hits(SearchKind::Grep, "query", "rs");
        assert_eq!(rs.len(), 3);
        assert!(rs.iter().all(|h| h.path.ends_with(".rs")));
        assert_eq!(super::hits(SearchKind::Grep, "query", ".md").len(), 1);
        // No matches is an honest empty state.
        assert!(super::hits(SearchKind::Grep, "zzz", "").is_empty());
    }

    #[test]
    fn mock_file_find_orders_best_match_first() {
        let hits = hits(SearchKind::FileFind, "query", "");
        assert_eq!(hits[0].path, "tests/query_roundtrip.rs");
    }
}
