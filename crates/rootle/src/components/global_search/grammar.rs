//! Rootle's literal search contract with strop-style qualifier tokenization.
//! Parsing and query coloring share one span lexer. Malformed queries fail
//! before provider I/O; unsupported qualifiers remain visible as diagnostics.
//! Content queries keep their raw wire spelling, not a regex translation.

use super::model::RawHit;
use ratatui::text::Span;

#[path = "query.rs"]
mod query;
pub(super) use crate::languages::path_ext;
pub use crate::languages::{ext_lang, lang_exts};
pub(super) use query::file_query;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grammar {
    pub terms: Vec<String>,
    pub negated: Vec<String>,
    pub languages: Vec<String>,
    pub negated_languages: Vec<String>,
    pub extensions: Vec<String>,
    pub negated_extensions: Vec<String>,
    pub paths: Vec<String>,
    pub negated_paths: Vec<String>,
    pub repos: Vec<String>,
    pub negated_repos: Vec<String>,
    pub orgs: Vec<String>,
    pub negated_orgs: Vec<String>,
    pub diagnostics: Vec<String>,
}

impl Grammar {
    pub fn validate(&self) -> rootle_provider::ProviderResult<()> {
        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(rootle_provider::ProviderError::other(
                self.diagnostics.join("; "),
            ))
        }
    }

    /// Positive values within a qualifier family are alternatives; different
    /// families intersect, and exclusions win, as in strop's file plan.
    pub fn allows_path(&self, path: &str) -> bool {
        let ext = path_ext(path);
        let language_ext = path.rsplit_once('.').map_or("", |(_, ext)| ext);
        let language_matches = |name: &String| {
            lang_exts(name).is_some_and(|extensions| extensions.contains(&language_ext))
        };
        self.diagnostics.is_empty()
            && (self.languages.is_empty() || self.languages.iter().any(language_matches))
            && !self.negated_languages.iter().any(language_matches)
            && (self.extensions.is_empty() || self.extensions.contains(&ext))
            && !self.negated_extensions.contains(&ext)
            && alternatives(&self.paths, &self.negated_paths, |value| {
                // strop path: is literal substring with smart case, not regex.
                if value.chars().any(char::is_uppercase) {
                    path.contains(value)
                } else {
                    path.to_lowercase().contains(value)
                }
            })
    }

    pub fn allows_repo(&self, repo: &str) -> bool {
        let org = repo.split('/').next().unwrap_or(repo);
        alternatives(&self.repos, &self.negated_repos, |value| value == repo)
            && alternatives(&self.orgs, &self.negated_orgs, |value| value == org)
    }
}

fn alternatives(positive: &[String], negative: &[String], matches: impl Fn(&str) -> bool) -> bool {
    (positive.is_empty() || positive.iter().any(|value| matches(value)))
        && !negative.iter().any(|value| matches(value))
}

pub fn parse(input: &str) -> Grammar {
    let mut result = Grammar::default();
    let mut negate_next = false;
    let mut pending_not = false;
    for token in query::lex(input) {
        if !token.complete {
            result.diagnostics.push("unclosed quote".into());
            continue;
        }
        if !token.quoted && token.text == "NOT" {
            negate_next = !negate_next;
            pending_not = true;
            continue;
        }
        let (prefix_negated, text) = if !token.quoted {
            token
                .text
                .strip_prefix('-')
                .map_or((false, token.text.as_str()), |s| (true, s))
        } else {
            (false, token.text.as_str())
        };
        let negated = negate_next ^ prefix_negated;
        negate_next = false;
        pending_not = false;
        if prefix_negated && text.is_empty() {
            result
                .diagnostics
                .push("negation expects a term or qualifier".into());
            continue;
        }
        if !token.quoted && matches!(text, "AND" | "OR") {
            result
                .diagnostics
                .push(format!("unsupported Boolean operator: {text}"));
            continue;
        }
        if let Some((key, value)) = (!token.quoted).then(|| query::qualifier(text)).flatten() {
            if value.is_empty() {
                result.diagnostics.push(format!("{key}: expects a value"));
                continue;
            }
            let target = match (key, negated) {
                ("language", _) if lang_exts(value).is_none() => {
                    result
                        .diagnostics
                        .push(format!("unknown language: {value}"));
                    continue;
                }
                ("language", false) => &mut result.languages,
                ("language", true) => &mut result.negated_languages,
                ("extension", false) => &mut result.extensions,
                ("extension", true) => &mut result.negated_extensions,
                ("path", false) => &mut result.paths,
                ("path", true) => &mut result.negated_paths,
                ("repo", false) => &mut result.repos,
                ("repo", true) => &mut result.negated_repos,
                ("org", false) => &mut result.orgs,
                ("org", true) => &mut result.negated_orgs,
                _ => {
                    result
                        .diagnostics
                        .push(format!("unsupported qualifier: {key}:"));
                    continue;
                }
            };
            if key == "extension" {
                let value = value.trim_start_matches('.').to_ascii_lowercase();
                if value.is_empty() {
                    result.diagnostics.push("extension: expects a value".into());
                } else {
                    target.push(value);
                }
            } else {
                target.push(value.to_string());
            }
        } else if negated {
            result.negated.push(text.to_lowercase());
        } else if !text.is_empty() {
            result.terms.push(text.to_string());
        }
    }
    if pending_not {
        result
            .diagnostics
            .push("NOT expects a term or qualifier".into());
    }
    result
}

pub fn style_query(input: &str, theme: &crate::theme::Theme) -> Vec<Span<'static>> {
    use ratatui::style::Style;
    let plain = Style::default().fg(theme.semantic.text);
    let warning = Style::default().fg(theme.syntax.invalid);
    let string = Style::default().fg(theme.syntax.string);
    let keyword = Style::default().fg(theme.syntax.keyword);
    let mut spans = Vec::new();
    let mut end = 0;
    for token in query::lex(input) {
        if token.range.start > end {
            spans.push(Span::styled(
                input[end..token.range.start].to_string(),
                plain,
            ));
        }
        end = token.range.end;
        let mut raw = &input[token.range];
        if !token.complete {
            spans.push(Span::styled(raw.to_string(), warning));
            continue;
        }
        if !token.quoted && raw.starts_with('-') {
            spans.push(Span::styled("-".to_string(), warning));
            raw = &raw[1..];
        }
        if !token.quoted && token.text == "NOT" {
            spans.push(Span::styled(raw.to_string(), warning));
        } else if !token.quoted
            && let Some((key, _)) = query::qualifier(token.text.trim_start_matches('-'))
            && query::known(key)
            && let Some((raw_key, raw_value)) = raw.split_once(':')
        {
            spans.push(Span::styled(format!("{raw_key}:"), keyword));
            spans.push(Span::styled(raw_value.to_string(), string));
        } else {
            let style = if raw.contains(['"', '\'']) {
                string
            } else {
                plain
            };
            spans.push(Span::styled(raw.to_string(), style));
        }
    }
    if end < input.len() {
        spans.push(Span::styled(input[end..].to_string(), plain));
    }
    spans
}

pub fn unexpressible(g: &Grammar) -> Vec<String> {
    g.diagnostics.clone()
}

/// Path predicates are exact; content exclusion is best-effort over available
/// preview lines. The filtered count reports this client-side subtraction.
pub fn filter_hits(g: &Grammar, hits: Vec<RawHit>) -> (Vec<RawHit>, usize) {
    let count = hits.len();
    let kept: Vec<_> = hits
        .into_iter()
        .filter(|hit| {
            g.allows_path(&hit.path)
                && g.allows_repo(&hit.repo)
                && !g.negated.iter().any(|needle| {
                    hit.path.to_lowercase().contains(needle)
                        || hit
                            .preview
                            .iter()
                            .any(|(_, line)| line.to_lowercase().contains(needle))
                })
        })
        .collect();
    let dropped = count - kept.len();
    (kept, dropped)
}

#[cfg(test)]
mod style_tests {
    use super::style_query;
    use crate::theme::Theme;

    fn colors(query: &str) -> Vec<(String, bool, bool, bool)> {
        let t = Theme::catppuccin_mocha();
        let (kw, str_, warn) = (t.syntax.keyword, t.syntax.string, t.syntax.invalid);
        style_query(query, &t)
            .into_iter()
            .map(|sp| {
                let st = sp.style;
                (
                    sp.content.to_string(),
                    st.fg == Some(kw),
                    st.fg == Some(str_),
                    st.fg == Some(warn),
                )
            })
            .collect()
    }

    #[test]
    fn qualifiers_literals_and_negation_take_syntax_colors() {
        let spans = colors(r#"render -legacy language:rust "exact phrase" NOT dead"#);
        // keyword-colored qualifier key, string-colored value.
        assert!(spans.iter().any(|(t, kw, _, _)| t == "language:" && *kw));
        assert!(spans.iter().any(|(t, _, st, _)| t == "rust" && *st));
        assert!(
            spans
                .iter()
                .any(|(t, _, st, _)| t == "\"exact phrase\"" && *st)
        );
        assert!(spans.iter().any(|(t, _, _, w)| t == "-" && *w));
        assert!(spans.iter().any(|(t, _, _, w)| t == "NOT" && *w));
        // Plain terms and spaces stay uncolored.
        assert!(
            spans
                .iter()
                .any(|(t, kw, st, w)| t == "render" && !kw && !st && !w)
        );
        assert!(
            spans
                .iter()
                .any(|(t, kw, st, w)| t == "legacy" && !kw && !st && !w)
        );
    }

    #[test]
    fn spans_partition_the_input_byte_exactly() {
        for q in [
            "",
            "plain",
            "a  b",
            "\"unterminated",
            "-\"neg quoted\"",
            "unknown:x",
            "path:src/main.rs render",
            "language:",
            "üñí — em",
        ] {
            let joined: String = style_query(q, &Theme::catppuccin_mocha())
                .iter()
                .map(|s| s.content.as_ref())
                .collect();
            assert_eq!(joined, q, "partition broke for {q:?}");
        }
        // An unknown qualifier is NOT a pill — plain text, no bleed.
        let spans = colors("unknown:x");
        assert!(spans.iter().all(|(_, kw, st, w)| !kw && !st && !w));
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str, preview: &[&str]) -> RawHit {
        RawHit {
            repo: "o/r".into(),
            path: path.into(),
            sha: "s".into(),
            branch: "main".into(),
            line: 1,
            preview: preview
                .iter()
                .enumerate()
                .map(|(i, l)| (i as u32 + 1, l.to_string()))
                .collect(),
            match_count: preview.len() as u32,
            stale: false,
        }
    }

    #[test]
    fn tokenizer_keeps_quoted_literals_whole() {
        let g = parse(r#"alpha "exact phrase" beta"#);
        assert_eq!(g.terms, ["alpha", "exact phrase", "beta"]);
        // Incomplete input is an error, never a broadened partial search.
        let g = parse(r#"a "b c"#);
        assert!(g.validate().is_err());
        assert!(!g.allows_path("anything.rs"));
    }

    #[test]
    fn negation_and_language_parse() {
        let g = parse(
            r#"render -deprecated NOT legacy -"dead code" language:rust -language:python extension:rs repo:o/r symbol:foo"#,
        );
        assert_eq!(g.terms, ["render"]);
        assert_eq!(g.negated, ["deprecated", "legacy", "dead code"]);
        assert_eq!(g.languages, ["rust"]);
        assert_eq!(g.negated_languages, ["python"]);
        assert_eq!(g.extensions, ["rs"]);
        assert_eq!(g.repos, ["o/r"]);
        assert_eq!(g.diagnostics, ["unsupported qualifier: symbol:"]);
    }

    #[test]
    fn filter_subtracts_negated_and_applies_language() {
        let g = parse("render -legacy language:rust");
        let hits = vec![
            hit("src/render.rs", &["fn render()"]),
            hit("src/legacy.rs", &["fn render()"]), // negated path
            hit("src/mod.rs", &["legacy render call"]), // negated preview
            hit("docs/render.md", &["render docs"]), // wrong language
        ];
        let (kept, dropped) = filter_hits(&g, hits);
        assert_eq!(dropped, 3);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "src/render.rs");
    }

    #[test]
    fn unknown_language_fails_closed_and_is_named() {
        let g = parse("render language:cobol");
        assert!(g.validate().is_err());
        let (kept, dropped) = filter_hits(&g, vec![hit("src/render.rs", &[])]);
        assert_eq!((kept.len(), dropped), (0, 1));
        assert_eq!(unexpressible(&g), ["unknown language: cobol"]);
    }

    #[test]
    fn ext_lang_reverses_the_same_table() {
        // Forward and reverse are one mapping: every extension of
        // every language resolves back to a name that resolves
        // forward to a list containing it.
        for row in rootle_stdio::SEARCH_LANGUAGES.lines() {
            let exts = row.split('\t').nth(2).unwrap();
            for ext in exts.split(',') {
                let back = ext_lang(ext).expect("every table ext resolves");
                assert!(
                    lang_exts(back).is_some_and(|xs| xs.contains(&ext)),
                    "{ext} → {back} must round-trip"
                );
            }
        }
        // Aliases resolve to the canonical name; unknowns don't.
        assert_eq!(ext_lang("cpp"), Some("cpp"));
        assert_eq!(ext_lang("cc"), Some("cpp"));
        assert_eq!(ext_lang("bash"), Some("shellscript"));
        assert_eq!(ext_lang("txt"), None);
        assert_eq!(lang_exts("cpp"), lang_exts("c++"));
        assert_eq!(lang_exts("cpp"), lang_exts("cxx"));
        assert!(lang_exts("CPP").is_none());
    }

    #[test]
    fn quoted_qualifiers_and_escapes_stay_literal() {
        let g = parse(r#""language:cpp" 'two words' "a\"b" 'C:\temp' language:"c++""#);
        assert!(g.validate().is_ok());
        assert_eq!(g.terms, ["language:cpp", "two words", "a\"b", r"C:\temp"]);
        assert_eq!(g.languages, ["c++"]);
    }

    #[test]
    fn families_union_exclusions_win_and_paths_never_match_content() {
        let g = parse("language:rs language:'c++' -extension:hh path:src -path:vendor");
        assert!(g.validate().is_ok());
        let hits = [
            "src/a.rs",
            "src/a.cp",
            "src/a.hxx",
            "src/a.hh",
            "src/vendor/a.cpp",
            "docs/a.cpp",
        ]
        .into_iter()
        .map(|path| hit(path, &["src"]))
        .collect();
        let (hits, dropped) = filter_hits(&g, hits);
        assert_eq!(dropped, 3);
        assert_eq!(
            hits.into_iter().map(|hit| hit.path).collect::<Vec<_>>(),
            ["src/a.rs", "src/a.cp", "src/a.hxx"]
        );
        assert!(!parse("language:cpp").allows_path("folder.cpp/README"));
        assert!(parse("language:rs").allows_path(".rs"));
        assert!(!parse("language:cpp").allows_path("src/a.CPP"));
        assert!(!parse("path:Source").allows_path("source/a.cpp"));
    }

    #[test]
    fn incomplete_or_unsupported_queries_never_broaden_results() {
        for input in [
            "language:",
            "language:cp",
            "extension:.",
            "'unfinished",
            "NOT",
            "NOT NOT",
            "-",
            "regex:x",
            "glob:*.rs",
            "a OR b",
        ] {
            let g = parse(input);
            assert!(g.validate().is_err(), "{input}");
            assert!(!g.allows_path("src/a.rs"), "{input}");
        }
    }

    #[test]
    fn namespace_urls_and_drive_prefixes_are_not_qualifiers() {
        let g = parse(r"std::vector https://example.test C:\temp x:y");
        assert!(g.validate().is_ok());
        assert_eq!(
            g.terms,
            [r"std::vector", "https://example.test", r"C:\temp", "x:y"]
        );
        assert_eq!(parse("path::suffix").paths, [":suffix"]);
    }
}
