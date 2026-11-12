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
        files: &[],
        inject: &["rust", "rs"],
    },
    Entry {
        lang: Lang::Python,
        label: "python",
        files: &[],
        inject: &["python", "py", "python3"],
    },
    Entry {
        lang: Lang::Javascript,
        label: "javascript",
        files: &[],
        inject: &["javascript", "js", "node"],
    },
    Entry {
        lang: Lang::Typescript,
        label: "typescript",
        files: &[],
        inject: &["typescript", "ts"],
    },
    Entry {
        lang: Lang::Tsx,
        label: "tsx",
        files: &[],
        inject: &["tsx"],
    },
    Entry {
        lang: Lang::Go,
        label: "go",
        files: &[],
        inject: &["go", "golang"],
    },
    Entry {
        lang: Lang::C,
        label: "c",
        files: &[],
        inject: &["c"],
    },
    Entry {
        lang: Lang::Cpp,
        label: "cpp",
        files: &[],
        inject: &["cpp", "c++"],
    },
    Entry {
        lang: Lang::Java,
        label: "java",
        files: &[],
        inject: &["java"],
    },
    Entry {
        lang: Lang::CSharp,
        label: "c#",
        files: &[],
        inject: &["csharp", "c#", "cs"],
    },
    Entry {
        lang: Lang::Ruby,
        label: "ruby",
        files: &["gemfile", "rakefile", "vagrantfile", "guardfile"],
        inject: &["ruby", "rb"],
    },
    Entry {
        lang: Lang::Php,
        label: "php",
        files: &[],
        inject: &["php"],
    },
    Entry {
        lang: Lang::Bash,
        label: "bash",
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
        files: &[],
        inject: &["lua"],
    },
    Entry {
        lang: Lang::Json,
        label: "json",
        files: &[],
        inject: &["json"],
    },
    Entry {
        lang: Lang::Toml,
        label: "toml",
        files: &[],
        inject: &["toml"],
    },
    Entry {
        lang: Lang::Yaml,
        label: "yaml",
        files: &[],
        inject: &["yaml", "yml"],
    },
    Entry {
        lang: Lang::Html,
        label: "html",
        files: &[],
        inject: &["html"],
    },
    Entry {
        lang: Lang::Css,
        label: "css",
        files: &[],
        inject: &["css"],
    },
    Entry {
        lang: Lang::Markdown,
        label: "markdown",
        files: &[],
        inject: &["markdown", "md"],
    },
    Entry {
        lang: Lang::MarkdownInline,
        label: "markdown",
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
    // TSX is a grammar specialization of the TypeScript search family.
    if ext.eq_ignore_ascii_case("tsx") {
        return Some(Lang::Tsx);
    }
    ENTRIES
        .iter()
        .filter(|e| !matches!(e.lang, Lang::Tsx | Lang::MarkdownInline))
        .find(|e| {
            crate::languages::lang_exts(e.label).is_some_and(|extensions| {
                extensions.iter().any(|name| name.eq_ignore_ascii_case(ext))
            })
        })
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
