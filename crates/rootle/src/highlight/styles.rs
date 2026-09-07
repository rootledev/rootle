//! Capture-name → palette-role mapping.
//!
//! Tree-sitter queries capture nodes with dotted names
//! (`keyword.function`, `string.escape`, …). `HighlightConfiguration::
//! configure` resolves every capture the queries use to the longest
//! recognized name below (ties go to the earlier entry), and the
//! highlighter reports that entry's index. `StyleTable` maps each index
//! onto the active theme; a theme switch rebuilds the table without
//! touching any query or parser state.

use ratatui::style::{Modifier, Style as RStyle};

use crate::theme::Theme;

/// How one recognized capture paints text. Markup roles compose a
/// palette role with a terminal modifier — markdown bold renders in
/// the constant color *and* bold.
enum Role {
    Keyword,
    String,
    Comment,
    Function,
    Type,
    Constant,
    Tag,
    Namespace,
    Invalid,
    /// `markup.bold` — constant color, bold.
    MarkupBold,
    /// `markup.italic` — keyword color, italic.
    MarkupItalic,
    /// `markup.strikethrough` — plain color, crossed out.
    MarkupStrike,
}

/// Recognized captures, in tie-break order. `keyword` precedes
/// `function` so `keyword.function` (`fn`, `def`) paints as a keyword;
/// `constructor` precedes `function` so constructor calls read as
/// calls. Captures absent from this list (plain `variable`, `property`,
/// `operator`, punctuation) intentionally fall back to the default
/// style — that mirrors the palette this app has always painted.
const CAPTURES: &[(&str, Role)] = &[
    // --- code ---
    ("keyword", Role::Keyword),
    ("conditional", Role::Keyword), // `if`/`else` (C/Python-era queries)
    ("repeat", Role::Keyword),      // `for`/`while`
    ("include", Role::Keyword),     // `#include`/`use`
    ("exception", Role::Keyword),   // `try`/`except`
    ("label", Role::Keyword),       // Rust lifetimes
    ("variable.builtin", Role::Keyword), // `self`/`this`
    ("string", Role::String),
    ("string.escape", Role::String),
    ("string.regexp", Role::String),
    ("string.special", Role::String),
    ("comment", Role::Comment),
    ("constant", Role::Constant), // covers constant.builtin/numeric
    ("number", Role::Constant),
    ("boolean", Role::Constant),
    ("constructor", Role::Function), // `Some(…)`, `new Foo()`
    ("function", Role::Function),
    ("attribute", Role::Function), // decorators/annotations
    ("type", Role::Type),          // covers type.builtin
    ("tag", Role::Tag),
    ("module", Role::Namespace),
    ("namespace", Role::Namespace), // pre-`module` queries
    ("error", Role::Invalid),       // parse errors
    ("invalid", Role::Invalid),     // invalid.illegal captures
    // --- markdown legacy capture names (tree-sitter-md still uses
    //     the pre-`markup.*` names: text.title, text.literal, …) ---
    ("text.title", Role::Tag),
    ("text.literal", Role::String),
    ("text.emphasis", Role::MarkupItalic),
    ("text.strong", Role::MarkupBold),
    ("text.uri", Role::String),
    ("punctuation.special", Role::Keyword), // heading/list markers
    // --- markup (markdown) ---
    ("markup.heading", Role::Tag),
    ("markup.raw", Role::String), // covers raw.inline / raw.block
    ("markup.bold", Role::MarkupBold),
    ("markup.italic", Role::MarkupItalic),
    ("markup.strikethrough", Role::MarkupStrike),
    ("markup.list", Role::Keyword),
    ("markup.quote", Role::Comment),
    ("markup.link.url", Role::String),
];

/// Capture names handed to `HighlightConfiguration::configure` — one
/// pass over `CAPTURES`, so the mapping table stays the single source
/// of truth.
pub(super) fn capture_names() -> Vec<&'static str> {
    CAPTURES.iter().map(|(name, _)| *name).collect()
}

/// A resolved style per capture slot plus the default style. Rebuilt
/// on theme switch; cheap (a few dozen `Style` copies) and never
/// recompiles a query.
pub(super) struct StyleTable {
    styles: Vec<RStyle>,
    default: RStyle,
}

impl StyleTable {
    pub(super) fn new(theme: &Theme) -> Self {
        let syntax = &theme.syntax;
        let fg = |color| RStyle::new().fg(color);
        let plain = fg(theme.semantic.text);
        StyleTable {
            default: plain,
            styles: CAPTURES
                .iter()
                .map(|(_, role)| match role {
                    Role::Keyword => fg(syntax.keyword),
                    Role::String => fg(syntax.string),
                    Role::Comment => fg(syntax.comment),
                    Role::Function => fg(syntax.function),
                    Role::Type => fg(syntax.type_),
                    Role::Constant => fg(syntax.constant),
                    Role::Tag => fg(syntax.tag),
                    Role::Namespace => fg(syntax.namespace),
                    Role::Invalid => fg(syntax.invalid),
                    Role::MarkupBold => fg(syntax.constant).add_modifier(Modifier::BOLD),
                    Role::MarkupItalic => fg(syntax.keyword).add_modifier(Modifier::ITALIC),
                    Role::MarkupStrike => plain.add_modifier(Modifier::CROSSED_OUT),
                })
                .collect(),
        }
    }

    /// Style for a capture slot reported by the highlighter; `None`
    /// (no active capture) paints with the default foreground.
    pub(super) fn style(&self, slot: Option<usize>) -> RStyle {
        match slot {
            Some(i) => self.styles.get(i).copied().unwrap_or(self.default),
            None => self.default,
        }
    }

    pub(super) fn default_style(&self) -> RStyle {
        self.default
    }
}
