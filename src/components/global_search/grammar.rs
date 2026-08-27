//! The search query grammar (plans/0012 M1): quoted literals, prefix
//! negation, and the `language:`/`extension:` qualifiers.
//!
//! The raw query still goes out on the wire verbatim — GitHub's
//! grammar is a superset natively, and stdio adapters translate what
//! they can. This structure is what ROOTLE itself needs: the local
//! tree file find and the client-side subtraction filter, plus the
//! honesty chips when a token can't be applied anywhere.

use super::model::RawHit;

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

/// Linguist-ish extension map for `language:` — small and static on
/// purpose. Unknown languages pass through unfiltered and land on the
/// title's `unfiltered` chip instead of silently widening the search.
pub fn lang_exts(lang: &str) -> Option<&'static [&'static str]> {
    Some(match lang.to_ascii_lowercase().as_str() {
        "rust" => &["rs"],
        "python" => &["py", "pyi"],
        "javascript" => &["js", "jsx", "mjs"],
        "typescript" => &["ts", "tsx", "mts"],
        "go" => &["go"],
        "c" => &["c", "h"],
        "c++" | "cpp" => &["cpp", "cc", "cxx", "hpp"],
        "csharp" | "c#" => &["cs"],
        "java" => &["java"],
        "kotlin" => &["kt", "kts"],
        "ruby" => &["rb"],
        "php" => &["php"],
        "swift" => &["swift"],
        "shell" | "bash" => &["sh", "bash"],
        "toml" => &["toml"],
        "yaml" => &["yaml", "yml"],
        "json" => &["json"],
        "markdown" => &["md"],
        "html" => &["html"],
        "css" => &["css"],
        _ => return None,
    })
}

fn path_ext(path: &str) -> String {
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
}
