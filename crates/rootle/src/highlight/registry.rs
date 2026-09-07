//! Language registry: filename detection and lazily compiled
//! highlight configurations.
//!
//! Grammars and queries are statically linked into the binary — the
//! `tree-sitter-*` crates ship C parsers and `.scm` queries compiled
//! in (musl-static friendly; nothing is downloaded or `dlopen`ed at
//! runtime). Compiling a grammar's queries costs milliseconds, so it
//! happens once per language on first use and the result is cached
//! for the process lifetime. Configurations are theme-independent:
//! colors are applied later from the capture index, so theme switches
//! never recompile anything.

use std::sync::OnceLock;

use tree_sitter_highlight::HighlightConfiguration;

use super::queries;

/// One registry language. TSX shares TypeScript's query (the grammar
/// is TS + JSX); C++ composes the C query (superset grammar);
/// TypeScript/TSX compose the JavaScript query, as the upstream
/// queries are layered supplements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Lang {
    Rust,
    Python,
    Javascript,
    Typescript,
    Tsx,
    Go,
    C,
    Cpp,
    Java,
    CSharp,
    Ruby,
    Php,
    Bash,
    Lua,
    Json,
    Toml,
    Yaml,
    Html,
    Css,
    Markdown,
    /// Inline parser for markdown prose (the block grammar injects it).
    MarkdownInline,
}

/// Everything needed to detect a language from a filename and to
/// resolve language-injection requests (markdown fences, HTML
/// `<script>`, Lua `ffi.cdef`, …).
pub(super) struct Entry {
    pub(super) lang: Lang,
    /// Footer label ("rust", "c#", …).
    label: &'static str,
    /// Lowercase extensions, no dot.
    exts: &'static [&'static str],
    /// Lowercase exact basenames (dotfiles, Gemfile, PKGBUILD…).
    files: &'static [&'static str],
    /// Lowercase names accepted from injection queries, after
    /// stripping `source.`/`text.` scope prefixes.
    inject: &'static [&'static str],
}

pub(super) const ENTRIES: &[Entry] = &[
    Entry {
        lang: Lang::Rust,
        label: "rust",
        exts: &["rs"],
        files: &[],
        inject: &["rust", "rs"],
    },
    Entry {
        lang: Lang::Python,
        label: "python",
        exts: &["py", "pyi", "pyw"],
        files: &[],
        inject: &["python", "py", "python3"],
    },
    Entry {
        lang: Lang::Javascript,
        label: "javascript",
        exts: &["js", "jsx", "mjs", "cjs"],
        files: &[],
        inject: &["javascript", "js", "node"],
    },
    Entry {
        lang: Lang::Typescript,
        label: "typescript",
        exts: &["ts", "mts", "cts"],
        files: &[],
        inject: &["typescript", "ts"],
    },
    Entry {
        lang: Lang::Tsx,
        label: "tsx",
        exts: &["tsx"],
        files: &[],
        inject: &["tsx"],
    },
    Entry {
        lang: Lang::Go,
        label: "go",
        exts: &["go"],
        files: &[],
        inject: &["go", "golang"],
    },
    Entry {
        lang: Lang::C,
        label: "c",
        exts: &["c", "h"],
        files: &[],
        inject: &["c"],
    },
    Entry {
        lang: Lang::Cpp,
        label: "cpp",
        exts: &["cpp", "cc", "cp", "cxx", "hpp", "hh", "hxx", "ino", "tpp"],
        files: &[],
        inject: &["cpp", "c++"],
    },
    Entry {
        lang: Lang::Java,
        label: "java",
        exts: &["java"],
        files: &[],
        inject: &["java"],
    },
    Entry {
        lang: Lang::CSharp,
        label: "c#",
        exts: &["cs", "csx"],
        files: &[],
        inject: &["csharp", "c#", "cs"],
    },
    Entry {
        lang: Lang::Ruby,
        label: "ruby",
        exts: &["rb", "rbw", "rake", "gemspec"],
        files: &["gemfile", "rakefile", "vagrantfile", "guardfile"],
        inject: &["ruby", "rb"],
    },
    Entry {
        lang: Lang::Php,
        label: "php",
        exts: &["php", "phtml"],
        files: &[],
        inject: &["php"],
    },
    Entry {
        lang: Lang::Bash,
        label: "bash",
        exts: &["sh", "bash", "zsh", "ksh"],
        files: &[
            ".bashrc",
            ".bash_profile",
            ".bash_aliases",
            ".bash_logout",
            ".zshrc",
            ".zshenv",
            ".zprofile",
            ".profile",
            "pkgbuild",
            "apkbuild",
        ],
        inject: &["bash", "sh", "shell", "zsh", "shell-script"],
    },
    Entry {
        lang: Lang::Lua,
        label: "lua",
        exts: &["lua"],
        files: &[],
        inject: &["lua"],
    },
    Entry {
        lang: Lang::Json,
        label: "json",
        exts: &["json", "jsonc"],
        files: &[],
        inject: &["json"],
    },
    Entry {
        lang: Lang::Toml,
        label: "toml",
        exts: &["toml"],
        files: &[],
        inject: &["toml"],
    },
    Entry {
        lang: Lang::Yaml,
        label: "yaml",
        exts: &["yaml", "yml"],
        files: &[],
        inject: &["yaml", "yml"],
    },
    Entry {
        lang: Lang::Html,
        label: "html",
        exts: &["html", "htm", "xhtml"],
        files: &[],
        inject: &["html"],
    },
    Entry {
        lang: Lang::Css,
        label: "css",
        exts: &["css"],
        files: &[],
        inject: &["css"],
    },
    Entry {
        lang: Lang::Markdown,
        label: "markdown",
        exts: &["md", "markdown"],
        files: &[],
        inject: &["markdown", "md"],
    },
    Entry {
        lang: Lang::MarkdownInline,
        label: "markdown",
        exts: &[],
        files: &[],
        inject: &["markdown_inline"],
    },
];

/// One configuration slot per registry language.
pub(super) const LANG_COUNT: usize = ENTRIES.len();

/// Detect a language from a filename (path or bare name). Exact
/// basename matches win (`.bashrc`, `Gemfile`), then extensions,
/// case-insensitively — nothing is read from the repository's disk.
pub(super) fn detect(filename: &str) -> Option<Lang> {
    let base = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    if let Some(entry) = ENTRIES
        .iter()
        .find(|e| e.files.iter().any(|name| name.eq_ignore_ascii_case(base)))
    {
        return Some(entry.lang);
    }
    let (_, ext) = base.rsplit_once('.').filter(|(stem, _)| !stem.is_empty())?;
    ENTRIES
        .iter()
        .find(|e| e.exts.iter().any(|name| name.eq_ignore_ascii_case(ext)))
        .map(|e| e.lang)
}

/// Footer label for a detected language.
pub(super) fn label(lang: Lang) -> &'static str {
    ENTRIES[lang as usize].label
}

/// The registry language for a name requested by an injection query
/// (markdown fence info string, `<script>`→"javascript", Lua
/// `ffi.cdef`→"c", …). Scope-prefixed names ("source.rust") normalize
/// to the bare name; unknown languages (`regex`, `jsdoc`, `sql`)
/// return `None` and the range keeps the outer document's captures.
pub(super) fn lang_for_injection(name: &str) -> Option<Lang> {
    let name = name.trim();
    let name = name
        .strip_prefix("source.")
        .or_else(|| name.strip_prefix("text."))
        .unwrap_or(name);
    ENTRIES
        .iter()
        .find(|e| {
            e.inject
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
        })
        .map(|e| e.lang)
}

/// Compiled configurations, cached for the process lifetime — one
/// compile per language, shared by every highlight call. Theme
/// switches never touch this cache.
static CONFIGS: [OnceLock<Option<HighlightConfiguration>>; LANG_COUNT] =
    [const { OnceLock::new() }; LANG_COUNT];

/// The compiled configuration for `lang`, compiling it on first use.
/// Returns `None` if the grammar's queries fail to compile — callers
/// degrade to plain text; the highlight tests pin that this never
/// happens for a registered language.
pub(super) fn config(lang: Lang) -> Option<&'static HighlightConfiguration> {
    CONFIGS[lang as usize]
        .get_or_init(|| queries::compile(lang).ok())
        .as_ref()
}
