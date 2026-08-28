//! The search query grammar (plans/0012 M1): quoted literals, prefix
//! negation, and the `language:`/`extension:` qualifiers.
//!
//! The raw query still goes out on the wire verbatim — GitHub's
//! grammar is a superset natively, and stdio adapters translate what
//! they can. This structure is what ROOTLE itself needs: the local
//! tree file find and the client-side subtraction filter, plus the
//! honesty chips when a token can't be applied anywhere.

use super::model::RawHit;
use ratatui::text::Span;

/// A parsed query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grammar {
    /// Positive terms; a quoted literal is ONE term (quotes stripped).
    pub terms: Vec<String>,
    /// Negated terms (`-foo`, `-"a b"`, `NOT foo`).
    pub negated: Vec<String>,
    /// `language:rust` — extension filter via the static table.
    pub language: Option<String>,
    /// `-language:rust` / `NOT language:rust`.
    pub negated_language: Option<String>,
    /// Inline `extension:rs` (the extension field's in-query twin).
    pub extension: Option<String>,
    /// Qualifiers rootle can't express anywhere (`symbol:`, `-repo:`…)
    /// — the results title names them.
    pub unknown: Vec<String>,
}

/// Whitespace split honoring double quotes; an unterminated quote
/// runs to the end of input (the phrase is still one term).
fn tokenize(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in q.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// Scope qualifiers: adapter/wire-level, never content needles —
/// parsed out so the local paths don't substring-match the syntax.
const SCOPE_QUALIFIERS: &[&str] = &["repo:", "org:", "path:"];

/// Syntax eye-candy for the query field (GitHub's qualifier pills):
/// qualifiers color the key in keyword and the value in string,
/// quoted literals keep their quotes in string, the negation marker is
/// warning-colored. The spans partition the input byte-exactly — a
/// query we can't segment falls through as one plain span, so nothing
/// ever bleeds or drops a character (tested invariant).
pub fn style_query(query: &str, theme: &crate::theme::Theme) -> Vec<Span<'static>> {
    let syntax = &theme.syntax;
    use ratatui::style::Style;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let text = Style::default().fg(theme.semantic.text);
    let bytes = query.as_bytes();
    let mut i = 0;
    let push = |spans: &mut Vec<Span<'static>>, s: &str, style: Style| {
        spans.push(Span::styled(s.to_string(), style));
    };
    while i < query.len() {
        let b = bytes[i];
        if b == b' ' || b == b'\t' {
            let start = i;
            while i < query.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            push(&mut spans, &query[start..i], text);
            continue;
        }
        // One token: quoted runs hold together, quotes stay visible.
        let start = i;
        while i < query.len() && bytes[i] != b' ' && bytes[i] != b'\t' {
            if bytes[i] == b'"' {
                i += 1;
                while i < query.len() && bytes[i] != b'"' {
                    i += 1;
                }
                if i < query.len() {
                    i += 1; // the closing quote
                }
            } else {
                i += 1;
            }
        }
        let tok = &query[start..i];
        let (neg, rest) = match tok.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, tok),
        };
        if neg {
            push(&mut spans, "-", Style::default().fg(syntax.invalid));
        }
        if tok == "NOT" {
            push(&mut spans, "NOT", Style::default().fg(syntax.invalid));
            continue;
        }
        let tok = rest;
        if tok.is_empty() {
            continue;
        }
        // Qualifier with a value → keyword key + string value.
        if let Some(colon) = tok.find(':')
            && !tok.starts_with('"')
        {
            let key = &tok[..=colon];
            let known = matches!(
                key,
                "repo:" | "org:" | "path:" | "extension:" | "language:" | "symbol:"
            );
            if known {
                push(&mut spans, key, Style::default().fg(syntax.keyword));
                let value = &tok[colon + 1..];
                if !value.is_empty() {
                    push(&mut spans, value, Style::default().fg(syntax.string));
                }
                continue;
            }
        }
        let style = if tok.starts_with('"') || tok.contains('"') {
            Style::default().fg(syntax.string)
        } else {
            text
        };
        push(&mut spans, tok, style);
    }
    debug_assert_eq!(
        spans.iter().map(|s| s.content.as_ref()).collect::<String>(),
        query,
        "style_query must partition the input"
    );
    spans
}

pub fn parse(query: &str) -> Grammar {
    let mut g = Grammar::default();
    let mut tokens = tokenize(query).into_iter();
    while let Some(tok) = tokens.next() {
        let (neg, tok) = if tok == "NOT" {
            match tokens.next() {
                Some(t) => (true, t),
                None => break,
            }
        } else if let Some(rest) = tok.strip_prefix('-') {
            (true, rest.to_string())
        } else {
            (false, tok)
        };
        if tok.is_empty() {
            continue;
        }
        if let Some(lang) = tok.strip_prefix("language:") {
            if neg {
                g.negated_language = Some(lang.to_string());
            } else {
                g.language = Some(lang.to_string());
            }
        } else if let Some(ext) = tok.strip_prefix("extension:") {
            if neg {
                g.unknown.push(format!("-{tok}"));
            } else {
                g.extension = Some(ext.trim_start_matches('.').to_string());
            }
        } else if SCOPE_QUALIFIERS.iter().any(|p| tok.starts_with(p)) {
            // Wire-level — the emitted q keeps them verbatim.
        } else if tok.contains(':') {
            g.unknown.push(if neg { format!("-{tok}") } else { tok });
        } else if neg {
            g.negated.push(tok.to_lowercase());
        } else {
            g.terms.push(tok);
        }
    }
    g
}

/// The one extension↔language mapping (plans/0012): the M1
/// `language:` qualifier resolves names → extensions, the M3 facet
/// chips resolve extensions → names. Two lookups over a single
/// table, so a new language can never land in one and not the other.
const LANG_TABLE: &[(&str, &[&str], &[&str])] = &[
    // (canonical name, aliases, extensions)
    ("rust", &[], &["rs"]),
    ("python", &[], &["py", "pyi"]),
    ("javascript", &[], &["js", "jsx", "mjs"]),
    ("typescript", &[], &["ts", "tsx", "mts"]),
    ("go", &[], &["go"]),
    ("c", &[], &["c", "h"]),
    ("c++", &["cpp"], &["cpp", "cc", "cxx", "hpp"]),
    ("csharp", &["c#"], &["cs"]),
    ("java", &[], &["java"]),
    ("kotlin", &[], &["kt", "kts"]),
    ("ruby", &[], &["rb"]),
    ("php", &[], &["php"]),
    ("swift", &[], &["swift"]),
    ("shell", &["bash"], &["sh", "bash"]),
    ("toml", &[], &["toml"]),
    ("yaml", &[], &["yaml", "yml"]),
    ("json", &[], &["json"]),
    ("markdown", &[], &["md"]),
    ("html", &[], &["html"]),
    ("css", &[], &["css"]),
];

/// Linguist-ish extension map for `language:` — small and static on
/// purpose. Unknown languages pass through unfiltered and land on the
/// title's `unfiltered` chip instead of silently widening the search.
pub fn lang_exts(lang: &str) -> Option<&'static [&'static str]> {
    let lower = lang.to_ascii_lowercase();
    LANG_TABLE
        .iter()
        .find(|(name, aliases, _)| *name == lower || aliases.contains(&lower.as_str()))
        .map(|(_, _, exts)| *exts)
}

/// Reverse lookup for the M3 facet chips: extension → canonical
/// language name, so a chip reads `rust` not `rs`. Same table as
/// `lang_exts` — there is no second mapping.
pub fn ext_lang(ext: &str) -> Option<&'static str> {
    LANG_TABLE
        .iter()
        .find(|(_, _, exts)| exts.contains(&ext))
        .map(|(name, _, _)| *name)
}

pub(super) fn path_ext(path: &str) -> String {
    path.rsplit('.').next().unwrap_or_default().to_lowercase()
}

pub(super) fn lang_matches(lang: &Option<String>, path: &str) -> Option<bool> {
    let lang = lang.as_ref()?;
    let exts = lang_exts(lang)?;
    Some(exts.contains(&path_ext(path).as_str()))
}

/// Tokens rootle couldn't apply anywhere — the title's honesty chip.
/// Unknown qualifiers, and `language:` values with no table entry.
pub fn unexpressible(g: &Grammar) -> Vec<String> {
    let mut out = g.unknown.clone();
    for (qual, lang) in [
        ("language:", &g.language),
        ("-language:", &g.negated_language),
    ] {
        if let Some(lang) = lang
            && lang_exts(lang).is_none()
        {
            out.push(format!("{qual}{lang}"));
        }
    }
    out
}

/// Client-side subtraction (plans/0012 M1): drop the hits a
/// grammar-incapable backend should have excluded. Exact on paths;
/// best-effort on content (preview lines only) — the `filtered` chip
/// owns that honesty. Returns (kept, dropped count). Backends that
/// applied the grammar natively (GitHub, a conforming adapter) lose
/// nothing: the filter is a no-op net over their sets.
pub fn filter_hits(g: &Grammar, hits: Vec<RawHit>) -> (Vec<RawHit>, usize) {
    if g.negated.is_empty() && g.language.is_none() && g.negated_language.is_none() {
        return (hits, 0);
    }
    let mut kept = Vec::with_capacity(hits.len());
    let mut dropped = 0;
    'hits: for hit in hits {
        let path = hit.path.to_lowercase();
        for needle in &g.negated {
            if path.contains(needle)
                || hit
                    .preview
                    .iter()
                    .any(|(_, line)| line.to_lowercase().contains(needle))
            {
                dropped += 1;
                continue 'hits;
            }
        }
        if let Some(false) = lang_matches(&g.language, &hit.path) {
            dropped += 1;
            continue;
        }
        if let Some(true) = lang_matches(&g.negated_language, &hit.path) {
            dropped += 1;
            continue;
        }
        kept.push(hit);
    }
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
        // Unterminated quote runs to the end.
        let g = parse(r#"a "b c"#);
        assert_eq!(g.terms, ["a", "b c"]);
    }

    #[test]
    fn negation_and_language_parse() {
        let g = parse(
            r#"render -deprecated NOT legacy -"dead code" language:rust -language:python extension:rs repo:o/r symbol:foo"#,
        );
        assert_eq!(g.terms, ["render"]);
        assert_eq!(g.negated, ["deprecated", "legacy", "dead code"]);
        assert_eq!(g.language.as_deref(), Some("rust"));
        assert_eq!(g.negated_language.as_deref(), Some("python"));
        assert_eq!(g.extension.as_deref(), Some("rs"));
        assert_eq!(g.unknown, ["symbol:foo"]); // repo: is wire-level, not unknown
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
    fn unknown_language_passes_through_and_is_named() {
        let g = parse("render language:cobol");
        let hits = vec![hit("src/render.rs", &[])];
        let (kept, dropped) = filter_hits(&g, hits);
        assert_eq!((kept.len(), dropped), (1, 0));
        assert_eq!(unexpressible(&g), ["language:cobol"]);
    }

    #[test]
    fn ext_lang_reverses_the_same_table() {
        // Forward and reverse are one mapping: every extension of
        // every language resolves back to a name that resolves
        // forward to a list containing it.
        for (_name, _, exts) in LANG_TABLE {
            for ext in *exts {
                let back = ext_lang(ext).expect("every table ext resolves");
                assert!(
                    lang_exts(back).is_some_and(|xs| xs.contains(ext)),
                    "{ext} → {back} must round-trip"
                );
            }
        }
        // Aliases resolve to the canonical name; unknowns don't.
        assert_eq!(ext_lang("cpp"), Some("c++"));
        assert_eq!(ext_lang("cc"), Some("c++"));
        assert_eq!(ext_lang("bash"), Some("shell"));
        assert_eq!(ext_lang("txt"), None);
        assert_eq!(lang_exts("CPP"), Some(&["cpp", "cc", "cxx", "hpp"][..]));
    }
}
