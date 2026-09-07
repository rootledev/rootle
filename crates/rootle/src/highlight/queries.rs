//! Upstream query composition and grammar-specific injection rules.

use std::sync::LazyLock;

use tree_sitter::Language;
use tree_sitter_highlight::HighlightConfiguration;

use super::registry::{Lang, label};
use super::styles;
/// Markdown block injections with the inline injection widened to
/// `injection.include-children`. The block grammar keeps the emphasis
/// delimiters (`*`, `` ` ``) as *children* of `(inline)` nodes, so the
/// upstream injection's default child-exclusion hands the inline
/// parser delimiter-less fragments — `*bold*` never forms. The
/// appended pattern re-injects the full node; the fragment-only
/// matches upstream stay but produce no captures.
static MARKDOWN_INJECTIONS: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{}\n((inline) @injection.content\n\
         \x20(#set! injection.language \"markdown_inline\")\n\
         \x20(#set! injection.include-children))\n",
        tree_sitter_md::INJECTION_QUERY_BLOCK
    )
});

/// Compose a language's (language, highlights, injections, locals).
/// Supplements concatenate, mirroring how editors layer the upstream
/// queries: TypeScript extends JavaScript, TSX adds the JSX patterns,
/// C++ extends C.
fn queries(lang: Lang) -> (Language, String, &'static str, String) {
    match lang {
        Lang::Rust => (
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY.into(),
            tree_sitter_rust::INJECTIONS_QUERY,
            String::new(),
        ),
        Lang::Python => (
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::Javascript => (
            tree_sitter_javascript::LANGUAGE.into(),
            format!(
                "{}{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            ),
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY.into(),
        ),
        Lang::Typescript | Lang::Tsx => {
            // The TS grammar has no JSX nodes; TSX adds them, so only
            // TSX gets the JSX patterns on top of the shared queries.
            let language = if lang == Lang::Typescript {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            } else {
                tree_sitter_typescript::LANGUAGE_TSX.into()
            };
            let jsx = if lang == Lang::Tsx {
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
            } else {
                ""
            };
            (
                language,
                format!(
                    "{}{}{}",
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    jsx,
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                ),
                "",
                format!(
                    "{}{}",
                    tree_sitter_javascript::LOCALS_QUERY,
                    tree_sitter_typescript::LOCALS_QUERY,
                ),
            )
        }
        Lang::Go => (
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::HIGHLIGHTS_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::C => (
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::Cpp => (
            tree_sitter_cpp::LANGUAGE.into(),
            // The C++ grammar is a superset of C; upstream ships only
            // the C++ delta on top of C's query.
            format!(
                "{}{}",
                tree_sitter_c::HIGHLIGHT_QUERY,
                tree_sitter_cpp::HIGHLIGHT_QUERY,
            ),
            "",
            String::new(),
        ),
        Lang::Java => (
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::HIGHLIGHTS_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::CSharp => (
            tree_sitter_c_sharp::LANGUAGE.into(),
            tree_sitter_c_sharp::HIGHLIGHTS_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::Ruby => (
            tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::HIGHLIGHTS_QUERY.into(),
            "",
            tree_sitter_ruby::LOCALS_QUERY.into(),
        ),
        Lang::Php => (
            tree_sitter_php::LANGUAGE_PHP.into(),
            tree_sitter_php::HIGHLIGHTS_QUERY.into(),
            tree_sitter_php::INJECTIONS_QUERY,
            String::new(),
        ),
        Lang::Bash => (
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::Lua => (
            tree_sitter_lua::LANGUAGE.into(),
            tree_sitter_lua::HIGHLIGHTS_QUERY.into(),
            tree_sitter_lua::INJECTIONS_QUERY,
            tree_sitter_lua::LOCALS_QUERY.into(),
        ),
        Lang::Json => (
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::Toml => (
            tree_sitter_toml_ng::LANGUAGE.into(),
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::Yaml => (
            tree_sitter_yaml::LANGUAGE.into(),
            tree_sitter_yaml::HIGHLIGHTS_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::Html => (
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY.into(),
            tree_sitter_html::INJECTIONS_QUERY,
            String::new(),
        ),
        Lang::Css => (
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY.into(),
            "",
            String::new(),
        ),
        Lang::Markdown => (
            tree_sitter_md::LANGUAGE.into(),
            tree_sitter_md::HIGHLIGHT_QUERY_BLOCK.into(),
            MARKDOWN_INJECTIONS.as_str(),
            String::new(),
        ),
        Lang::MarkdownInline => (
            tree_sitter_md::INLINE_LANGUAGE.into(),
            tree_sitter_md::HIGHLIGHT_QUERY_INLINE.into(),
            tree_sitter_md::INJECTION_QUERY_INLINE,
            String::new(),
        ),
    }
}
pub(super) fn compile(lang: Lang) -> Result<HighlightConfiguration, tree_sitter::QueryError> {
    let (language, highlights, injections, locals) = queries(lang);
    let mut config =
        HighlightConfiguration::new(language, label(lang), &highlights, injections, &locals)?;
    config.configure(&styles::capture_names());
    Ok(config)
}
