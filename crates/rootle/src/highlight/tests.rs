use super::*;
use crate::theme::Theme;
use ratatui::style::{Color, Modifier};

/// Mocha syntax roles, for readable assertions.
fn mocha() -> Theme {
    Theme::catppuccin_mocha()
}

/// Concatenate a rendered line's spans — the fidelity oracle: styled
/// output must reproduce the source text byte for byte.
fn joined(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>()
}

/// Observe token styling without depending on how spans were coalesced.
fn span_style(lines: &[ratatui::text::Line<'_>], needle: &str) -> ratatui::style::Style {
    for line in lines {
        let Some(start) = joined(line).find(needle) else {
            continue;
        };
        let end = start + needle.len();
        let mut offset = 0;
        let mut style = None;
        for span in &line.spans {
            let span_end = offset + span.content.len();
            if offset < end && span_end > start {
                if let Some(previous) = style {
                    assert_eq!(previous, span.style, "nonuniform style for {needle:?}");
                } else {
                    style = Some(span.style);
                }
            }
            offset = span_end;
        }
        return style.expect("token has source characters");
    }
    panic!("missing token {needle:?}: {lines:?}");
}

/// One file per registry language, exercising each grammar's core
/// constructs. The fidelity test renders all of them; samples include
/// the constructs most likely to break on query drift.
fn sample(lang: registry::Lang) -> (&'static str, &'static str) {
    use registry::Lang;
    match lang {
        Lang::Rust => ("lib.rs", "fn main() { let s = \"héllo\\n\"; }\n"),
        Lang::Python => ("app.py", "def f(x):\n    return x + 1\n"),
        Lang::Javascript => ("app.js", "const el = <div a=\"b\">hi</div>;\n"),
        Lang::Typescript => ("app.ts", "const x: number = 1;\n"),
        Lang::Tsx => ("app.tsx", "const el = <div a=\"b\">hi</div>;\n"),
        Lang::Go => ("main.go", "func main() { fmt.Println(\"hi\") }\n"),
        Lang::C => (
            "main.c",
            "#include <stdio.h>\nint main(void) { return 0; }\n",
        ),
        Lang::Cpp => (
            "main.cpp",
            "namespace n { auto f() -> int { return 0; } }\n",
        ),
        Lang::Java => ("App.java", "class App { int f() { return 0; } }\n"),
        Lang::CSharp => ("App.cs", "class App { int F() => 0; }\n"),
        Lang::Ruby => ("app.rb", "def f(x)\n  \"#{x}!\"\nend\n"),
        Lang::Php => ("app.php", "<?php\nfunction f($x) { return $x; }\n"),
        Lang::Bash => ("run.sh", "echo \"hi\"\ncd /tmp\n"),
        Lang::Lua => ("app.lua", "local function f(x) return x end\n"),
        Lang::Json => ("data.json", "{\"k\": [1, true, null]}\n"),
        Lang::Toml => ("Cargo.toml", "[package]\nname = \"x\"\n"),
        Lang::Yaml => ("ci.yaml", "jobs:\n  build:\n    - run: make\n"),
        Lang::Html => ("page.html", "<p class=\"x\">hi</p>\n"),
        Lang::Css => ("page.css", "p { color: red; }\n"),
        Lang::Markdown => ("README.md", "# Title\n\nsome *bold* text\n"),
        Lang::MarkdownInline => ("inline.md", "*b* and [l](https://x)\n"),
    }
}

// --- registry invariants -------------------------------------------------

#[test]
fn all_registered_queries_initialize() {
    // Every grammar's composed queries (including the TS/TSX/C++
    // supplements over JavaScript/C) must compile against its grammar.
    for entry in registry::ENTRIES {
        assert!(
            registry::config(entry.lang).is_some(),
            "{:?} queries failed to compile",
            entry.lang
        );
    }
}

#[test]
fn every_language_renders_faithfully() {
    let h = Highlighter::new(&mocha());
    for entry in registry::ENTRIES {
        // MarkdownInline is only reachable through markdown's
        // injection; its config compiles (test above) and its output
        // is covered by the injection tests below.
        if entry.lang == registry::Lang::MarkdownInline {
            continue;
        }
        let (name, text) = sample(entry.lang);
        let lines = h.highlight(name, text);
        assert_eq!(
            lines.len(),
            text.lines().count(),
            "{name}: line count must match text.lines()"
        );
        for (line, original) in lines.iter().zip(text.lines()) {
            assert_eq!(
                joined(line),
                original,
                "{name}: spans must reproduce the source line"
            );
        }
    }
}

// --- detection -----------------------------------------------------------

#[test]
fn language_label_follows_extension() {
    let h = Highlighter::default();
    assert_eq!(h.language("lib.rs"), "rust");
    assert_eq!(h.language("Cargo.toml"), "toml");
    assert_eq!(h.language("src/app.ts"), "typescript");
    assert_eq!(h.language("widget.tsx"), "tsx");
    assert_eq!(h.language("README.MD"), "markdown"); // case-insensitive
    assert_eq!(h.language(".bashrc"), "bash"); // dotfile basename
    assert_eq!(h.language("Gemfile"), "ruby");
    assert_eq!(h.language("data.xyz123"), "text");
    assert_eq!(h.language("noext"), "text");
}

#[test]
fn unknown_extension_renders_plain() {
    let h = Highlighter::new(&mocha());
    let lines = h.highlight("data.xyz123", "just text\n\ttabbed\n");
    assert_eq!(lines.len(), 2);
    assert_eq!(joined(&lines[0]), "just text");
    assert_eq!(joined(&lines[1]), "    tabbed");
    // Plain lines still carry the theme's text color (like the
    // plain-text syntax did).
    assert_eq!(lines[0].spans[0].style.fg, Some(mocha().semantic.text));
}

// --- palette mapping -----------------------------------------------------

#[test]
fn rust_keywords_get_mauve() {
    let h = Highlighter::default();
    let lines = h.highlight("lib.rs", "fn main() {}\n");
    assert_eq!(lines.len(), 1);
    // "fn" should be colored (mocha mauve 203,166,247), not default text.
    let style = span_style(&lines, "fn");
    assert_eq!(style.fg, Some(Color::Rgb(203, 166, 247)));
}

#[test]
fn theme_switch_recolors_keywords() {
    // The app's restyle path: one highlighter, set_theme, re-render.
    let mut h = Highlighter::default();
    h.set_theme(&Theme::embedded("dracula").unwrap());
    let lines = h.highlight("lib.rs", "fn main() {}\n");
    // dracula keyword = pink (255,121,198) — not mocha mauve.
    assert_eq!(span_style(&lines, "fn").fg, Some(Color::Rgb(255, 121, 198)));
}

#[test]
fn strings_and_comments_follow_roles() {
    let h = Highlighter::new(&mocha());
    let text = "// note\nlet s = \"x\";\n";
    let lines = h.highlight("lib.rs", text);
    assert_eq!(
        span_style(&lines, "// note").fg,
        Some(mocha().syntax.comment)
    );
    assert_eq!(span_style(&lines, "\"x\"").fg, Some(mocha().syntax.string));
}

#[test]
fn tabs_expand_inside_spans() {
    let h = Highlighter::default();
    let lines = h.highlight("main.rs", "\tfn x() {}\n");
    let joined = joined(&lines[0]);
    assert_eq!(joined, "    fn x() {}");
    assert!(!joined.contains('\t'));
}

// --- line fidelity -------------------------------------------------------

#[test]
fn multiline_constructs_keep_context() {
    let h = Highlighter::new(&mocha());
    // A block comment spanning lines: every line stays comment-colored
    // (whole-file parse), and the trailing newline yields no phantom
    // empty line — matching `text.lines()`.
    let text = "/* one\ntwo\nthree */\nlet x = 1;\n";
    let lines = h.highlight("lib.rs", text);
    assert_eq!(lines.len(), text.lines().count());
    for line in &lines[..3] {
        assert_eq!(
            line.spans[0].style.fg,
            Some(mocha().syntax.comment),
            "block-comment line keeps the comment color"
        );
    }
    // The line after the comment: `let` keeps the keyword color while
    // the surrounding unstyled text (`x`, `=`, `;`) falls back to the
    // default foreground (adjacent same-style pieces merge).
    assert_eq!(
        span_style(&lines[3..], "let").fg,
        Some(mocha().syntax.keyword)
    );
    let x = lines[3]
        .spans
        .iter()
        .find(|s| s.content.contains('x'))
        .expect("x present");
    assert_eq!(x.style.fg, Some(mocha().semantic.text));
}

#[test]
fn crlf_and_trailing_line_semantics() {
    let h = Highlighter::default();
    let lines = h.highlight("a.rs", "let a = 1;\r\nlet b = 2;\r\n");
    assert_eq!(lines.len(), 2); // like `str::lines`
    assert_eq!(joined(&lines[0]), "let a = 1;"); // \r stripped
    assert_eq!(joined(&lines[1]), "let b = 2;");
    assert!(lines.iter().all(|l| !joined(l).contains('\r')));

    // Trailing blank semantics mirror `lines()` exactly.
    assert_eq!(h.highlight("a.rs", "x\n").len(), 1);
    assert_eq!(h.highlight("a.rs", "x\n\n").len(), 2);
    assert_eq!(h.highlight("a.rs", "\n").len(), 1);
    assert!(h.highlight("a.rs", "").is_empty());
}

#[test]
fn unicode_text_survives_byte_ranges() {
    let h = Highlighter::new(&mocha());
    // Multibyte content: byte offsets from the parser must slice on
    // char boundaries and never lose or duplicate text.
    let text = "let s = \"日本語🎉ok\";\n// é́ comment\n";
    let lines = h.highlight("lib.rs", text);
    for (line, original) in lines.iter().zip(text.lines()) {
        assert_eq!(joined(line), original);
    }
    assert_eq!(
        span_style(&lines, "\"日本語🎉ok\"").fg,
        Some(mocha().syntax.string)
    );
}

#[test]
fn nested_captures_innermost_wins() {
    let h = Highlighter::new(&mocha());
    // The `\n` escape is captured inside the string capture; the
    // escape keeps the string role (not the outer default), and the
    // whole literal stays one color.
    let lines = h.highlight("lib.rs", "let s = \"a\\nb\";\n");
    assert_eq!(joined(&lines[0]), "let s = \"a\\nb\";");
    assert_eq!(
        span_style(&lines, "\"a\\nb\"").fg,
        Some(mocha().syntax.string)
    );
}

// --- injections ----------------------------------------------------------

#[test]
fn markdown_fences_inject_fenced_language() {
    let h = Highlighter::new(&mocha());
    let md = "# Title\n\n```rust\nfn main() {}\n```\n";
    let lines = h.highlight("README.md", md);
    assert_eq!(lines.len(), md.lines().count());
    // The fence body renders as rust: `fn` gets the keyword color,
    // not markdown raw-string color and not plain text.
    let fence = &lines[3];
    assert_eq!(joined(fence), "fn main() {}");
    assert_eq!(
        span_style(std::slice::from_ref(fence), "fn").fg,
        Some(mocha().syntax.keyword)
    );
    // And the heading still uses the block grammar's title capture.
    assert_eq!(span_style(&lines, "Title").fg, Some(mocha().syntax.tag));
}

#[test]
fn markdown_inline_grammar_handles_emphasis() {
    let h = Highlighter::new(&mocha());
    let lines = h.highlight("README.md", "a *em* and **strong** b\n");
    // Span text includes the delimiters; the injected markdown_inline
    // parser paints `*…*` as emphasis (keyword color + italic) and
    // `**…**` as strong (constant color + bold).
    let em = span_style(&lines, "*em*");
    assert_eq!(em.fg, Some(mocha().syntax.keyword));
    assert!(em.add_modifier.contains(Modifier::ITALIC));
    let strong = span_style(&lines, "**strong**");
    assert_eq!(strong.fg, Some(mocha().syntax.constant));
    assert!(strong.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn html_scripts_inject_javascript() {
    let h = Highlighter::new(&mocha());
    let html = "<script>var x = 1;</script>\n";
    let lines = h.highlight("page.html", html);
    assert_eq!(joined(&lines[0]), html.trim_end());
    assert_eq!(span_style(&lines, "var").fg, Some(mocha().syntax.keyword));
    assert_eq!(span_style(&lines, "script").fg, Some(mocha().syntax.tag));
}
