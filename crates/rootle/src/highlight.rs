//! Tree-sitter highlighting mapped onto the active palette (PLAN.md §11).
//!
//! Grammars are statically linked: every parser and highlight query
//! compiles into the binary (`tree-sitter-*` grammar crates,
//! musl-static friendly — no runtime downloads, no `dlopen`).
//! Supported languages: Rust, Python, JavaScript/JSX, TypeScript,
//! TSX, Go, C, C++, Java, C#, Ruby, PHP, Bash, Lua, JSON, TOML,
//! YAML, HTML, CSS and Markdown — fenced code blocks inject the
//! fenced language, and markdown prose gets its own inline parser
//! (tree-sitter-md's split grammars).
//!
//! Capture colors come from the `Theme`'s syntax roles, so highlighting
//! follows theme switches (embedded palettes and `themes/<name>.toml`
//! alike). Compiling a grammar's queries is the expensive half; the
//! registry caches configurations for the process lifetime and a theme
//! switch only swaps the color table.

mod queries;
mod registry;
mod render;
mod styles;
#[cfg(test)]
mod tests;

use std::cell::RefCell;

use ratatui::text::Line;

use crate::theme::Theme;

pub struct Highlighter {
    styles: styles::StyleTable,
    parser: RefCell<tree_sitter_highlight::Highlighter>,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new(&Theme::catppuccin_mocha())
    }
}

impl Highlighter {
    /// Build a highlighter for the effective theme. Grammar
    /// configurations compile lazily on first use, cached
    /// process-wide.
    pub fn new(theme: &Theme) -> Self {
        Highlighter {
            styles: styles::StyleTable::new(theme),
            parser: RefCell::new(tree_sitter_highlight::Highlighter::new()),
        }
    }

    /// Swap the color table without touching the cached grammar
    /// configurations (theme switch; the expensive half — query
    /// compilation — is never repeated).
    pub fn set_theme(&mut self, theme: &Theme) {
        self.styles = styles::StyleTable::new(theme);
    }

    /// Language label for the preview footer ("rust", "markdown", …).
    /// Unknown extensions fall back to "text".
    pub fn language(&self, filename: &str) -> String {
        registry::detect(filename)
            .map(|lang| registry::label(lang).to_owned())
            .unwrap_or_else(|| "text".into())
    }

    /// Highlight `text` as the syntax for `filename`'s extension.
    /// The whole buffer is parsed at once, so multiline constructs
    /// (doc comments, block strings, heredocs) keep their context.
    /// Unknown extensions render as plain text (no panic, no
    /// highlight). Tabs expand to four spaces per span — raw `\t`
    /// jumps to terminal stops and breaks column alignment (the
    /// plain-text preview path expands the same way).
    pub fn highlight(&self, filename: &str, text: &str) -> Vec<Line<'static>> {
        let Some(lang) = registry::detect(filename) else {
            return render::plain(text, &self.styles);
        };
        // A compile failure (grammar/query mismatch) degrades to
        // plain text rather than panicking on the UI thread; the
        // registry test pins that this never happens.
        let Some(config) = registry::config(lang) else {
            return render::plain(text, &self.styles);
        };

        let mut highlighter = self.parser.borrow_mut();
        let events = highlighter.highlight(config, text.as_bytes(), None, |name| {
            registry::lang_for_injection(name).and_then(registry::config)
        });
        match events {
            // `map_while` stops at the first error; `render::styled`
            // fills any uncovered remainder with the default style.
            Ok(events) => render::styled(text, events.map_while(Result::ok), &self.styles),
            Err(_) => render::plain(text, &self.styles),
        }
    }
}
