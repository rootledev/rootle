//! syntect highlighting mapped onto the active palette (PLAN.md §11).
//! Pure-Rust fancy-regex backend (musl-static friendly). The syntect
//! theme is built from the `Theme`'s syntax roles, so it follows
//! theme switches (embedded palettes and `themes/<name>.toml` alike).

use ratatui::style::{Color as RColor, Modifier, Style as RStyle};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SColor, FontStyle, ScopeSelectors, StyleModifier, Theme as STheme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::SyntaxSet;

use crate::theme::Theme;

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: STheme,
}

/// ratatui → syntect color. Roles are always `Rgb` (from_u32); any
/// other variant degrades to white rather than panicking.
fn scolor(c: RColor) -> SColor {
    match c {
        RColor::Rgb(r, g, b) => SColor { r, g, b, a: 255 },
        _ => SColor {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
    }
}

/// Build a syntect theme from the app theme's syntax roles. Span
/// backgrounds stay unset — the terminal/pane background shows through.
fn syntect_theme(theme: &Theme) -> STheme {
    let syn = &theme.syntax;
    let text = scolor(theme.semantic.text);

    let rule = |scope: &str, fg: RColor| ThemeItem {
        scope: scope.parse::<ScopeSelectors>().expect("scope selector"),
        style: StyleModifier {
            foreground: Some(scolor(fg)),
            background: None,
            font_style: None,
        },
    };

    STheme {
        name: Some("rootle-palette".into()),
        author: Some("rootle".into()),
        settings: ThemeSettings {
            foreground: Some(text),
            ..Default::default()
        },
        scopes: vec![
            rule("keyword, storage", syn.keyword),
            rule("string, string.quoted", syn.string),
            rule("comment, punctuation.definition.comment", syn.comment),
            rule(
                "entity.name.function, support.function, meta.function-call",
                syn.function,
            ),
            rule(
                "entity.name.type, support.type, entity.name.struct, entity.name.enum",
                syn.type_,
            ),
            rule("constant.numeric, constant.language", syn.constant),
            rule("entity.name.tag, markup.heading", syn.tag),
            rule("variable.parameter, variable.other", theme.semantic.text),
            rule("entity.name.namespace, meta.path", syn.namespace),
            rule("markup.bold, punctuation.definition.bold", syn.constant),
            rule("markup.italic", syn.keyword),
            rule("markup.raw, markup.fenced_code", syn.string),
            rule("invalid, invalid.illegal", syn.invalid),
        ],
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new(&Theme::catppuccin_mocha())
    }
}

impl Highlighter {
    /// The syntect theme is derived from the app theme; rebuild the
    /// highlighter when the effective theme changes.
    pub fn new(theme: &Theme) -> Self {
        Highlighter {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme: syntect_theme(theme),
        }
    }

    /// Swap the color table without reloading the syntax set (theme
    /// switch; the expensive half — the syntax dump — is untouched).
    pub fn set_theme(&mut self, theme: &Theme) {
        self.theme = syntect_theme(theme);
    }

    /// Language label for the preview footer ("rust", "markdown", …).
    /// Unknown extensions fall back to "text".
    pub fn language(&self, filename: &str) -> String {
        self.syntaxes
            .find_syntax_for_file(filename)
            .ok()
            .flatten()
            .map(|s| s.name.to_lowercase())
            .unwrap_or_else(|| "text".into())
    }

    /// Highlight `text` as the syntax for `filename`'s extension.
    /// Unknown extensions render as plain text (no panic, no highlight).
    /// Tabs expand to four spaces per span — raw `\t` jumps to terminal
    /// stops and breaks column alignment (the plain-text preview path
    /// expands the same way).
    pub fn highlight(&self, filename: &str, text: &str) -> Vec<Line<'static>> {
        let syntax = self
            .syntaxes
            .find_syntax_for_file(filename)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());

        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        text.lines()
            .map(|line| {
                let regions = match highlighter.highlight_line(line, &self.syntaxes) {
                    Ok(r) => r,
                    Err(_) => {
                        return Line::from(Span::raw(line.replace('\t', "    ")));
                    }
                };
                Line::from(
                    regions
                        .into_iter()
                        .map(|(style, text)| {
                            let fg = style.foreground;
                            let mut rstyle =
                                RStyle::default().fg(ratatui::style::Color::Rgb(fg.r, fg.g, fg.b));
                            if style.font_style.contains(FontStyle::BOLD) {
                                rstyle = rstyle.add_modifier(Modifier::BOLD);
                            }
                            Span::styled(text.replace('\t', "    "), rstyle)
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_keywords_get_mauve() {
        let h = Highlighter::default();
        let lines = h.highlight("lib.rs", "fn main() {}\n");
        assert_eq!(lines.len(), 1);
        // "fn" should be colored (mocha mauve 203,166,247), not default text.
        let first = &lines[0].spans[0];
        assert_eq!(
            first.style.fg,
            Some(ratatui::style::Color::Rgb(203, 166, 247)),
            "fn keyword should be mauve, got {:?}",
            first.style.fg
        );
    }

    #[test]
    fn theme_switch_recolors_keywords() {
        let dracula = Highlighter::new(&Theme::embedded("dracula").unwrap());
        let lines = dracula.highlight("lib.rs", "fn main() {}\n");
        // dracula keyword = pink (255,121,198) — not mocha mauve.
        assert_eq!(
            lines[0].spans[0].style.fg,
            Some(ratatui::style::Color::Rgb(255, 121, 198)),
            "fn keyword should follow the dracula palette, got {:?}",
            lines[0].spans[0].style.fg
        );
    }

    #[test]
    fn tabs_expand_inside_spans() {
        let h = Highlighter::default();
        let lines = h.highlight("main.rs", "\tfn x() {}\n");
        let joined: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "    fn x() {}");
        assert!(!joined.contains('\t'));
    }

    #[test]
    fn language_label_follows_extension() {
        let h = Highlighter::default();
        assert_eq!(h.language("lib.rs"), "rust");
        // The default syntax set ships no TOML grammar — plain text.
        assert_eq!(h.language("Cargo.toml"), "text");
        assert_eq!(h.language("data.xyz123"), "text");
    }

    #[test]
    fn unknown_extension_renders_plain() {
        let h = Highlighter::default();
        let lines = h.highlight("data.xyz123", "just text\n");
        assert_eq!(lines.len(), 1);
    }
}
