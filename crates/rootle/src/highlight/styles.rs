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
    Number,
    Attribute,
    Operator,
    Punctuation,
    Variable,
    Property,
    Heading,
    Link,
    Code,
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
/// calls. Fine-grained roles fall back to the original palette mapping
/// when a theme does not define them.
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
    ("number", Role::Number),
    ("constant.numeric", Role::Number),
    ("boolean", Role::Constant),
    ("constructor", Role::Function), // `Some(…)`, `new Foo()`
    ("function", Role::Function),
    ("attribute", Role::Attribute),
    ("type", Role::Type), // covers type.builtin
    ("tag", Role::Tag),
    ("module", Role::Namespace),
    ("namespace", Role::Namespace), // pre-`module` queries
    ("error", Role::Invalid),       // parse errors
    ("invalid", Role::Invalid),     // invalid.illegal captures
    ("operator", Role::Operator),
    ("punctuation", Role::Punctuation),
    ("variable", Role::Variable),
    ("property", Role::Property),
    ("variable.member", Role::Property),
    // --- markdown legacy capture names (tree-sitter-md still uses
    //     the pre-`markup.*` names: text.title, text.literal, …) ---
    ("text.title", Role::Heading),
    ("text.literal", Role::Code),
    ("text.emphasis", Role::MarkupItalic),
    ("text.strong", Role::MarkupBold),
    ("text.uri", Role::Link),
    ("punctuation.special", Role::Keyword), // heading/list markers
    // --- markup (markdown) ---
    ("markup.heading", Role::Heading),
    ("markup.raw", Role::Code),
    ("markup.bold", Role::MarkupBold),
    ("markup.italic", Role::MarkupItalic),
    ("markup.strikethrough", Role::MarkupStrike),
    ("markup.list", Role::Keyword),
    ("markup.quote", Role::Comment),
    ("markup.link.url", Role::Link),
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
                    Role::Comment => fg(syntax.comment).add_modifier(Modifier::ITALIC),
                    Role::Function => fg(syntax.function),
                    Role::Type => fg(syntax.type_),
                    Role::Constant => fg(syntax.constant),
                    Role::Tag => fg(syntax.tag),
                    Role::Namespace => fg(syntax.namespace),
                    Role::Invalid => fg(syntax.invalid),
                    Role::Number => fg(syntax.number.unwrap_or(syntax.constant)),
                    Role::Attribute => fg(syntax.attribute.unwrap_or(syntax.function)),
                    Role::Operator => fg(syntax.operator.unwrap_or(theme.semantic.text)),
                    Role::Punctuation => fg(syntax.punctuation.unwrap_or(theme.semantic.text)),
                    Role::Variable => fg(syntax.variable.unwrap_or(theme.semantic.text)),
                    Role::Property => fg(syntax.property.unwrap_or(theme.semantic.text)),
                    Role::Heading => fg(syntax.heading.unwrap_or(syntax.tag)),
                    Role::Link => {
                        fg(syntax.link.unwrap_or(syntax.string)).add_modifier(Modifier::UNDERLINED)
                    }
                    Role::Code => fg(syntax.code.unwrap_or(syntax.string)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn capture(theme: &Theme, name: &str) -> RStyle {
        let index = CAPTURES.iter().position(|(n, _)| *n == name).unwrap();
        StyleTable::new(theme).style(Some(index))
    }

    #[test]
    fn hone_distinguishes_numbers_constants_and_punctuation() {
        let theme = Theme::hone();
        assert_eq!(
            capture(&theme, "number").fg,
            Some(Color::from_u32(0xe39a7b))
        );
        assert_eq!(
            capture(&theme, "constant").fg,
            Some(Color::from_u32(0xe8a848))
        );
        assert_eq!(
            capture(&theme, "punctuation").fg,
            Some(Color::from_u32(0x9da3af))
        );
        assert!(
            capture(&theme, "comment")
                .add_modifier
                .contains(Modifier::ITALIC)
        );
        assert!(
            capture(&theme, "text.uri")
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn legacy_roles_and_overrides_remain_the_fallback() {
        let mut theme = Theme::catppuccin_mocha();
        theme.syntax.constant = Color::Red;
        theme.semantic.text = Color::Blue;
        assert_eq!(capture(&theme, "number").fg, Some(Color::Red));
        assert_eq!(capture(&theme, "punctuation").fg, Some(Color::Blue));
        theme.syntax.number = Some(Color::Green);
        assert_eq!(capture(&theme, "number").fg, Some(Color::Green));
        assert_eq!(capture(&theme, "constant").fg, Some(Color::Red));
    }
}
