//! Themes: Catppuccin Mocha is the embedded default; the other famous
//! dark palettes (Dracula, One Dark, Gruvbox Dark, Nord, Tokyo Night,
//! Solarized Dark) plus four light ones (Catppuccin Latte, GitHub
//! Light, One Light, Solarized Light) ship embedded too.
//! `~/.config/rootle/themes/<name>.toml` overrides merge on top of the
//! embedded base — fork a builtin by writing a file with its name.

mod diff;
mod palettes;

use ratatui::style::Color;
use ratatui::widgets::BorderType;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub semantic: Semantic,
    pub syntax: Syntax,
    /// Border corner style for panes and popups (chrome, not color —
    /// rides on `Theme` because that is what every render receives).
    pub border: BorderShape,
    /// Nerd Font glyphs in chrome (powerline arrows, forge icons);
    /// false = unicode fallbacks. Same ride-along as `border`.
    pub nerd_font: bool,
    /// Modeline chip separator when Nerd Font is off: "pipe" (`|`,
    /// rectangular chips) or "caret" (❯). Same ride-along.
    pub separator: SeparatorShape,
}

/// The modeline's chip separator shape ([ui] separator).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeparatorShape {
    /// `|` — rectangular chips, reads clean on every font.
    #[default]
    Pipe,
    /// ❯ — the starship-style caret.
    Caret,
}

impl SeparatorShape {
    /// Config-string parse; unknown values keep the default.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pipe" => Some(Self::Pipe),
            "caret" => Some(Self::Caret),
            _ => None,
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Self::Pipe => "|",
            Self::Caret => "\u{276f}",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Semantic {
    pub base: Color,
    pub mantle: Color,
    pub crust: Color,
    pub surface0: Color,
    pub surface2: Color,
    pub overlay0: Color,
    pub text: Color,
    pub subtext0: Color,

    pub border_focused: Color,
    pub border_unfocused: Color,
    pub directory: Color,
    pub file: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub hint: Color,
    pub error: Color,
    pub warning: Color,

    pub mode_browse: Color,
    pub mode_search: Color,
    pub mode_insert: Color,
    pub mode_normal: Color,
    pub mode_leader: Color,
    pub mode_visual: Color,

    /// Background of the modeline's forge chip (active provider
    /// identity); fg is `crust` like every chip. Defaults to each
    /// palette's overlay/muted tone — quiet next to the mode chip.
    pub forge: Color,

    pub badge_repo: Color,
    pub badge_org: Color,

    /// Background of a grep match inside a preview line (fg = crust).
    pub search_match: Color,

    /// Diff rows (plans/0028): quiet full-row tints carry the
    /// add/del signal; the `_strong` pair is the intra-line emphasis
    /// (delta's two-tier idea); `diff_band` sits under structural
    /// rows (stats, hunk headers). Defaults follow strop's tuned
    /// Mocha values.
    pub diff_add_fg: Color,
    pub diff_del_fg: Color,
    pub diff_add_bg: Color,
    pub diff_del_bg: Color,
    pub diff_add_strong: Color,
    pub diff_del_strong: Color,
    pub diff_band: Color,
}

/// Syntax-highlight roles mapped from Tree-sitter captures in `highlight/`.
/// Kept separate from `Semantic` — chrome colors and code colors evolve
/// on different axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Syntax {
    pub keyword: Color,
    pub string: Color,
    pub comment: Color,
    pub function: Color,
    pub type_: Color,
    pub constant: Color,
    pub tag: Color,
    pub namespace: Color,
    pub invalid: Color,
}

/// Corner style for pane/popup borders; `[ui] border` in the config
/// selects it. `Plain` is the default: square `┌─┐` corners join the
/// straight segments in the same stroke and read crisp at any font
/// size, while `Rounded`'s arc glyphs render soft or blurry in many
/// terminal fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderShape {
    #[default]
    Plain,
    Rounded,
    Thick,
    Double,
}

impl BorderShape {
    /// Config-string parse; unknown values keep the default.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "plain" => Some(Self::Plain),
            "rounded" => Some(Self::Rounded),
            "thick" => Some(Self::Thick),
            "double" => Some(Self::Double),
            _ => None,
        }
    }

    pub fn border_type(self) -> BorderType {
        match self {
            Self::Plain => BorderType::Plain,
            Self::Rounded => BorderType::Rounded,
            Self::Thick => BorderType::Thick,
            Self::Double => BorderType::Double,
        }
    }
}

/// One role override as a (name, hex) pair — the embedded palettes and
/// Catppuccin Mocha, the baseline. Unlisted roles in the other palettes
/// inherit from here — every embedded theme is complete anyway, the
/// inheritance exists for future partial palettes.
impl Theme {
    pub fn catppuccin_mocha() -> Self {
        Theme {
            semantic: Semantic {
                crust: Color::from_u32(0x11111b),
                mantle: Color::from_u32(0x181825),
                base: Color::from_u32(0x1e1e2e),
                surface0: Color::from_u32(0x313244),
                surface2: Color::from_u32(0x585b70),
                overlay0: Color::from_u32(0x6c7086),
                subtext0: Color::from_u32(0xa6adc8),
                text: Color::from_u32(0xcdd6f4),

                border_focused: Color::from_u32(0x89b4fa), // blue — dominant accent
                border_unfocused: Color::from_u32(0x585b70), // surface2
                directory: Color::from_u32(0x89b4fa),      // blue
                file: Color::from_u32(0xcdd6f4),           // text
                selection_bg: Color::from_u32(0x313244),   // surface0
                selection_fg: Color::from_u32(0x89b4fa),   // blue
                hint: Color::from_u32(0xa6adc8),           // subtext0
                error: Color::from_u32(0xf38ba8),          // red
                warning: Color::from_u32(0xf9e2af),        // yellow

                mode_browse: Color::from_u32(0xa6e3a1), // green
                mode_search: Color::from_u32(0xf9e2af), // yellow
                mode_insert: Color::from_u32(0x94e2d5), // teal
                mode_normal: Color::from_u32(0x89b4fa), // blue
                mode_leader: Color::from_u32(0xfab387), // peach
                mode_visual: Color::from_u32(0xf5c2e7), // pink

                forge: Color::from_u32(0x6c7086), // overlay0

                badge_repo: Color::from_u32(0x89b4fa), // blue
                badge_org: Color::from_u32(0xfab387),  // peach

                search_match: Color::from_u32(0xf9e2af), // yellow
                diff_add_fg: Color::from_u32(0xa9c47c),
                diff_del_fg: Color::from_u32(0xe8677a),
                diff_add_bg: Color::from_u32(0x1b2620),
                diff_del_bg: Color::from_u32(0x2a1d20),
                diff_add_strong: Color::from_u32(0x273a30),
                diff_del_strong: Color::from_u32(0x402a2e),
                diff_band: Color::from_u32(0x22242e),
            },
            border: BorderShape::default(),
            nerd_font: false,
            separator: SeparatorShape::Pipe,
            syntax: Syntax {
                keyword: Color::from_u32(0xcba6f7),   // mauve
                string: Color::from_u32(0xa6e3a1),    // green
                comment: Color::from_u32(0x6c7086),   // overlay0
                function: Color::from_u32(0x89b4fa),  // blue
                type_: Color::from_u32(0xf9e2af),     // yellow
                constant: Color::from_u32(0xfab387),  // peach
                tag: Color::from_u32(0xf38ba8),       // red
                namespace: Color::from_u32(0x94e2d5), // teal
                invalid: Color::from_u32(0xf38ba8),   // red
            },
        }
    }

    /// The embedded palettes, by name. `None` for unknown names.
    pub fn embedded(name: &str) -> Option<Self> {
        palettes::EMBEDDED
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, roles, syntax)| {
                let mut theme = Self::catppuccin_mocha();
                for &(role, hex) in *roles {
                    set_role(&mut theme.semantic, role, Color::from_u32(hex));
                }
                for &(role, hex) in *syntax {
                    set_syntax_role(&mut theme.syntax, role, Color::from_u32(hex));
                }
                if name != "catppuccin-mocha" {
                    diff::apply(&mut theme.semantic, |name| {
                        roles.iter().any(|(role, _)| *role == name)
                    });
                }
                theme
            })
    }

    /// Apply a config-selected border shape (chained after `load`).
    pub fn with_border(mut self, shape: BorderShape) -> Self {
        self.border = shape;
        self
    }

    /// The ratatui border type every pane/popup renders with.
    pub fn border_type(&self) -> BorderType {
        self.border.border_type()
    }

    /// Enable Nerd Font glyphs (chained after `load`).
    pub fn with_nerd_font(mut self, on: bool) -> Self {
        self.nerd_font = on;
        self
    }

    /// Modeline separator from `[ui] separator` (chained after
    /// `load`); unknown values keep the pipe.
    pub fn with_separator(mut self, sep: &str) -> Self {
        if let Some(shape) = SeparatorShape::parse(sep) {
            self.separator = shape;
        }
        self
    }

    pub fn available_names() -> Vec<String> {
        let mut names: Vec<String> = palettes::EMBEDDED
            .iter()
            .map(|(n, _, _)| n.to_string())
            .collect();
        if let Some(dir) =
            rootle_provider::paths::config_dir().map(|d| d.join("rootle").join("themes"))
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == "toml")
                    && let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str())
                {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }

    /// Load the named theme: the embedded palette when known (Mocha
    /// otherwise), merged with `~/.config/rootle/themes/<name>.toml`
    /// overrides (missing file, malformed TOML, unknown roles, bad hex
    /// → silently keep defaults; theming must never crash the app).
    pub fn load(name: &str) -> Self {
        let mut theme = Self::embedded(name).unwrap_or_else(Self::catppuccin_mocha);
        let Some(dir) = rootle_provider::paths::config_dir() else {
            return theme;
        };
        let path = dir
            .join("rootle")
            .join("themes")
            .join(format!("{name}.toml"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            return theme;
        };
        let Ok(overrides) = toml::from_str::<ThemeOverrides>(&text) else {
            return theme;
        };
        overrides.apply(&mut theme);
        theme
    }
}

/// Palette file: `[semantic]` and `[syntax]` role overrides.
#[derive(Debug, Default, serde::Deserialize)]
struct ThemeOverrides {
    #[serde(default)]
    semantic: std::collections::HashMap<String, String>,
    #[serde(default)]
    syntax: std::collections::HashMap<String, String>,
}

fn parse_hex(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    u32::from_str_radix(hex, 16).ok().map(Color::from_u32)
}

/// One syntax-role assignment; shared by palette files and embedded
/// palettes. `type` maps to `Syntax::type_` (TOML keys are strings,
/// so the reserved word is fine on disk).
fn set_syntax_role(syn: &mut Syntax, role: &str, color: Color) {
    match role {
        "keyword" => syn.keyword = color,
        "string" => syn.string = color,
        "comment" => syn.comment = color,
        "function" => syn.function = color,
        "type" => syn.type_ = color,
        "constant" => syn.constant = color,
        "tag" => syn.tag = color,
        "namespace" => syn.namespace = color,
        "invalid" => syn.invalid = color,
        _ => {} // unknown role: ignored, not an error
    }
}

/// One role assignment; shared by palette files and embedded palettes.
fn set_role(sem: &mut Semantic, role: &str, color: Color) {
    match role {
        "crust" => sem.crust = color,
        "mantle" => sem.mantle = color,
        "base" => sem.base = color,
        "surface0" => sem.surface0 = color,
        "surface2" => sem.surface2 = color,
        "overlay0" => sem.overlay0 = color,
        "subtext0" => sem.subtext0 = color,
        "text" => sem.text = color,
        "border_focused" => sem.border_focused = color,
        "border_unfocused" => sem.border_unfocused = color,
        "directory" => sem.directory = color,
        "file" => sem.file = color,
        "selection_bg" => sem.selection_bg = color,
        "selection_fg" => sem.selection_fg = color,
        "hint" => sem.hint = color,
        "error" => sem.error = color,
        "warning" => sem.warning = color,
        "mode_browse" => sem.mode_browse = color,
        "mode_search" => sem.mode_search = color,
        "mode_insert" => sem.mode_insert = color,
        "mode_normal" => sem.mode_normal = color,
        "mode_leader" => sem.mode_leader = color,
        "mode_visual" => sem.mode_visual = color,
        "forge" => sem.forge = color,
        "badge_repo" => sem.badge_repo = color,
        "badge_org" => sem.badge_org = color,
        "search_match" => sem.search_match = color,
        "diff_add_fg" => sem.diff_add_fg = color,
        "diff_del_fg" => sem.diff_del_fg = color,
        "diff_add_bg" => sem.diff_add_bg = color,
        "diff_del_bg" => sem.diff_del_bg = color,
        "diff_add_strong" => sem.diff_add_strong = color,
        "diff_del_strong" => sem.diff_del_strong = color,
        "diff_band" => sem.diff_band = color,
        _ => {} // unknown role: ignored, not an error
    }
}

impl ThemeOverrides {
    fn apply(self, theme: &mut Theme) {
        for (role, hex) in &self.semantic {
            if let Some(color) = parse_hex(hex) {
                set_role(&mut theme.semantic, role, color);
            }
        }
        if ["base", "surface0", "mode_browse", "error"]
            .iter()
            .any(|role| self.semantic.contains_key(*role))
        {
            diff::apply(&mut theme.semantic, |role| self.semantic.contains_key(role));
        }
        for (role, hex) in self.syntax {
            if let Some(color) = parse_hex(&hex) {
                set_syntax_role(&mut theme.syntax, &role, color);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Embedded palettes. Each lists every role explicitly — Mocha defaults
// are the compile-time safety net, not the intent. Values follow each
// palette's published spec, mapped onto the app's semantic roles.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
