//! syntect highlighting mapped onto the active palette (PLAN.md §11).
//! Pure-Rust fancy-regex backend (musl-static friendly). The syntect
//! theme is built programmatically from the Mocha palette roles; when
//! external palettes land, this mapping consumes them.

use ratatui::style::{Modifier, Style as RStyle};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SColor, FontStyle, ScopeSelectors, StyleModifier, Theme as STheme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::SyntaxSet;

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: STheme,
}

fn rgb(hex: u32) -> SColor {
    SColor {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
        a: 255,
    }
}

/// Build a syntect theme from Catppuccin Mocha (until external palette
/// files land — then these hexes come from the palette).
fn mocha_syntect_theme() -> STheme {
    // Mocha hexes (mirrors theme.rs; will be palette-driven).
    const BASE: u32 = 0x1e1e2e;
    const TEXT: u32 = 0xcdd6f4;
    const OVERLAY: u32 = 0x6c7086;
    const MAUVE: u32 = 0xcba6f7;
    const BLUE: u32 = 0x89b4fa;
    const GREEN: u32 = 0xa6e3a1;
    const YELLOW: u32 = 0xf9e2af;
    const PEACH: u32 = 0xfab387;
    const RED: u32 = 0xf38ba8;
    const TEAL: u32 = 0x94e2d5;

    let rule = |scope: &str, fg: u32| ThemeItem {
        scope: scope.parse::<ScopeSelectors>().expect("scope selector"),
        style: StyleModifier {
            foreground: Some(rgb(fg)),
            background: None,
            font_style: None,
        },
    };

    STheme {
        name: Some("catppuccin-mocha".into()),
        author: Some("rootle".into()),
        settings: ThemeSettings {
            foreground: Some(rgb(TEXT)),
            background: Some(rgb(BASE)),
            ..Default::default()
        },
        scopes: vec![
            rule("keyword, storage", MAUVE),
            rule("string, string.quoted", GREEN),
            rule("comment, punctuation.definition.comment", OVERLAY),
            rule(
                "entity.name.function, support.function, meta.function-call",
                BLUE,
            ),
            rule(
                "entity.name.type, support.type, entity.name.struct, entity.name.enum",
                YELLOW,
            ),
            rule("constant.numeric, constant.language", PEACH),
            rule("entity.name.tag, markup.heading", RED),
            rule("variable.parameter, variable.other", TEXT),
            rule("entity.name.namespace, meta.path", TEAL),
            rule("markup.bold, punctuation.definition.bold", PEACH),
            rule("markup.italic", MAUVE),
            rule("markup.raw, markup.fenced_code", GREEN),
            rule("invalid, invalid.illegal", RED),
        ],
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl Highlighter {
    /// Palette parameter reserved for when external themes land; the
    /// syntect mapping already takes a `&Theme` so the seam exists.
    pub fn new() -> Self {
        Highlighter {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme: mocha_syntect_theme(),
        }
    }

    /// Highlight `text` as the syntax for `filename`'s extension.
    /// Unknown extensions render as plain text (no panic, no highlight).
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
                        return Line::from(Span::raw(line.to_string()));
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
                            Span::styled(text.to_string(), rstyle)
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
        let h = Highlighter::new();
        let lines = h.highlight("lib.rs", "fn main() {}\n");
        assert_eq!(lines.len(), 1);
        // "fn" should be colored (mauve 203,166,247), not default text.
        let first = &lines[0].spans[0];
        assert_eq!(
            first.style.fg,
            Some(ratatui::style::Color::Rgb(203, 166, 247)),
            "fn keyword should be mauve, got {:?}",
            first.style.fg
        );
    }

    #[test]
    fn unknown_extension_renders_plain() {
        let h = Highlighter::new();
        let lines = h.highlight("data.xyz123", "just text\n");
        assert_eq!(lines.len(), 1);
    }
}
