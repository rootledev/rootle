//! Themes: Catppuccin Mocha is the embedded default; the other famous
//! dark palettes (Dracula, One Dark, Gruvbox Dark, Nord, Tokyo Night,
//! Solarized Dark) ship embedded too. `~/.config/rootle/themes/<name>.toml`
//! overrides merge on top of the embedded base — fork a builtin by
//! writing a file with its name.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub semantic: Semantic,
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

    pub badge_repo: Color,
    pub badge_org: Color,

    /// Background of a grep match inside a preview line (fg = crust).
    pub search_match: Color,
}

/// One role override as a (name, hex) pair — the embedded palettes and
/// palette files share this shape via `set_role`.
type RoleValue = (&'static str, u32);

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

                badge_repo: Color::from_u32(0x89b4fa), // blue
                badge_org: Color::from_u32(0xfab387),  // peach

                search_match: Color::from_u32(0xf9e2af), // yellow
            },
        }
    }

    /// The embedded palettes, by name. `None` for unknown names.
    pub fn embedded(name: &str) -> Option<Self> {
        EMBEDDED.iter().find(|(n, _)| *n == name).map(|(_, roles)| {
            let mut theme = Self::catppuccin_mocha();
            for &(role, hex) in *roles {
                set_role(&mut theme.semantic, role, Color::from_u32(hex));
            }
            theme
        })
    }

    /// Every theme name the loader can resolve: embedded palettes plus
    /// any `themes/<name>.toml` in the config dir. Settings list.
    pub fn available_names() -> Vec<String> {
        let mut names: Vec<String> = EMBEDDED.iter().map(|(n, _)| n.to_string()).collect();
        if let Some(dir) = dirs::config_dir().map(|d| d.join("rootle").join("themes"))
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
        let Some(dir) = dirs::config_dir() else {
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

/// Palette file: only `[semantic]` role overrides for now.
#[derive(Debug, Default, serde::Deserialize)]
struct ThemeOverrides {
    #[serde(default)]
    semantic: std::collections::HashMap<String, String>,
}

fn parse_hex(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    u32::from_str_radix(hex, 16).ok().map(Color::from_u32)
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
        "badge_repo" => sem.badge_repo = color,
        "badge_org" => sem.badge_org = color,
        "search_match" => sem.search_match = color,
        _ => {} // unknown role: ignored, not an error
    }
}

impl ThemeOverrides {
    fn apply(self, theme: &mut Theme) {
        for (role, hex) in self.semantic {
            if let Some(color) = parse_hex(&hex) {
                set_role(&mut theme.semantic, &role, color);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Embedded palettes. Each lists every role explicitly — Mocha defaults
// are the compile-time safety net, not the intent. Values follow each
// palette's published spec, mapped onto the app's semantic roles.
// ---------------------------------------------------------------------------

const DRACULA: &[RoleValue] = &[
    ("crust", 0x191a21),
    ("mantle", 0x21222c),
    ("base", 0x282a36),
    ("surface0", 0x44475a),
    ("surface2", 0x6272a4),
    ("overlay0", 0x6272a4),
    ("subtext0", 0x9aa2c7),
    ("text", 0xf8f8f2),
    ("border_focused", 0xbd93f9), // purple
    ("border_unfocused", 0x6272a4),
    ("directory", 0x8be9fd), // cyan
    ("file", 0xf8f8f2),
    ("selection_bg", 0x44475a),
    ("selection_fg", 0xbd93f9),
    ("hint", 0x6272a4),
    ("error", 0xff5555),
    ("warning", 0xffb86c),     // orange
    ("mode_browse", 0x50fa7b), // green
    ("mode_search", 0xf1fa8c), // yellow
    ("mode_insert", 0x8be9fd),
    ("mode_normal", 0xbd93f9),
    ("mode_leader", 0xffb86c),
    ("mode_visual", 0xff79c6), // pink
    ("badge_repo", 0x8be9fd),
    ("badge_org", 0xffb86c),
    ("search_match", 0xf1fa8c),
];

const ONE_DARK: &[RoleValue] = &[
    ("crust", 0x1b1f27),
    ("mantle", 0x21252b),
    ("base", 0x282c34),
    ("surface0", 0x3e4451),
    ("surface2", 0x5c6370),
    ("overlay0", 0x5c6370),
    ("subtext0", 0x828997),
    ("text", 0xabb2bf),
    ("border_focused", 0x61afef), // blue
    ("border_unfocused", 0x5c6370),
    ("directory", 0x61afef),
    ("file", 0xabb2bf),
    ("selection_bg", 0x3e4451),
    ("selection_fg", 0x61afef),
    ("hint", 0x828997),
    ("error", 0xe06c75),
    ("warning", 0xe5c07b),     // yellow
    ("mode_browse", 0x98c379), // green
    ("mode_search", 0xe5c07b),
    ("mode_insert", 0x56b6c2), // cyan
    ("mode_normal", 0x61afef),
    ("mode_leader", 0xd19a66), // orange
    ("mode_visual", 0xc678dd), // purple
    ("badge_repo", 0x61afef),
    ("badge_org", 0xd19a66),
    ("search_match", 0xe5c07b),
];

const GRUVBOX_DARK: &[RoleValue] = &[
    ("crust", 0x141618),
    ("mantle", 0x1d2021),
    ("base", 0x282828),
    ("surface0", 0x3c3836),
    ("surface2", 0x504945),
    ("overlay0", 0x928374),
    ("subtext0", 0xbdae93),
    ("text", 0xebdbb2),
    ("border_focused", 0x83a598), // blue
    ("border_unfocused", 0x504945),
    ("directory", 0x83a598),
    ("file", 0xebdbb2),
    ("selection_bg", 0x3c3836),
    ("selection_fg", 0x83a598),
    ("hint", 0xbdae93),
    ("error", 0xfb4934),
    ("warning", 0xfabd2f),     // yellow
    ("mode_browse", 0xb8bb26), // green
    ("mode_search", 0xfabd2f),
    ("mode_insert", 0x8ec07c), // aqua
    ("mode_normal", 0x83a598),
    ("mode_leader", 0xfe8019), // orange
    ("mode_visual", 0xd3869b), // purple
    ("badge_repo", 0x83a598),
    ("badge_org", 0xfe8019),
    ("search_match", 0xfabd2f),
];

const NORD: &[RoleValue] = &[
    ("crust", 0x242933),
    ("mantle", 0x272c36),
    ("base", 0x2e3440),
    ("surface0", 0x434c5e),
    ("surface2", 0x4c566a),
    ("overlay0", 0x616e88),
    ("subtext0", 0x9aa4b8),
    ("text", 0xd8dee9),
    ("border_focused", 0x88c0d0), // frost 8
    ("border_unfocused", 0x4c566a),
    ("directory", 0x88c0d0),
    ("file", 0xd8dee9),
    ("selection_bg", 0x434c5e),
    ("selection_fg", 0x88c0d0),
    ("hint", 0x9aa4b8),
    ("error", 0xbf616a),       // aurora red
    ("warning", 0xebcb8b),     // aurora yellow
    ("mode_browse", 0xa3be8c), // aurora green
    ("mode_search", 0xebcb8b),
    ("mode_insert", 0x8fbcbb), // frost 7
    ("mode_normal", 0x81a1c1), // frost 9
    ("mode_leader", 0xd08770), // aurora orange
    ("mode_visual", 0xb48ead), // aurora purple
    ("badge_repo", 0x88c0d0),
    ("badge_org", 0xd08770),
    ("search_match", 0xebcb8b),
];

const TOKYO_NIGHT: &[RoleValue] = &[
    ("crust", 0x12131a),
    ("mantle", 0x16161e),
    ("base", 0x1a1b26),
    ("surface0", 0x292e42),
    ("surface2", 0x3b4261),
    ("overlay0", 0x565f89),
    ("subtext0", 0xa9b1d6),
    ("text", 0xc0caf5),
    ("border_focused", 0x7aa2f7), // blue
    ("border_unfocused", 0x3b4261),
    ("directory", 0x7dcfff), // cyan
    ("file", 0xc0caf5),
    ("selection_bg", 0x292e42),
    ("selection_fg", 0x7aa2f7),
    ("hint", 0xa9b1d6),
    ("error", 0xf7768e),
    ("warning", 0xe0af68),     // yellow
    ("mode_browse", 0x9ece6a), // green
    ("mode_search", 0xe0af68),
    ("mode_insert", 0x7dcfff),
    ("mode_normal", 0x7aa2f7),
    ("mode_leader", 0xff9e64), // orange
    ("mode_visual", 0xbb9af7), // purple
    ("badge_repo", 0x7aa2f7),
    ("badge_org", 0xff9e64),
    ("search_match", 0xe0af68),
];

const SOLARIZED_DARK: &[RoleValue] = &[
    ("crust", 0x001f27),
    ("mantle", 0x073642), // base02 — raised surfaces
    ("base", 0x002b36),   // base03
    ("surface0", 0x073642),
    ("surface2", 0x586e75),       // base01
    ("overlay0", 0x657b83),       // base00
    ("subtext0", 0x839496),       // base0
    ("text", 0x93a1a1),           // base1
    ("border_focused", 0x268bd2), // blue
    ("border_unfocused", 0x586e75),
    ("directory", 0x268bd2),
    ("file", 0x93a1a1),
    ("selection_bg", 0x073642),
    ("selection_fg", 0x268bd2),
    ("hint", 0x839496),
    ("error", 0xdc322f),
    ("warning", 0xb58900),     // yellow
    ("mode_browse", 0x859900), // green
    ("mode_search", 0xb58900),
    ("mode_insert", 0x2aa198), // cyan
    ("mode_normal", 0x268bd2),
    ("mode_leader", 0xcb4b16), // orange
    ("mode_visual", 0xd33682), // magenta
    ("badge_repo", 0x268bd2),
    ("badge_org", 0xcb4b16),
    ("search_match", 0xb58900),
];

const EMBEDDED: &[(&str, &[RoleValue])] = &[
    ("catppuccin-mocha", &[]), // baseline, constructed directly
    ("dracula", DRACULA),
    ("gruvbox-dark", GRUVBOX_DARK),
    ("nord", NORD),
    ("one-dark", ONE_DARK),
    ("solarized-dark", SOLARIZED_DARK),
    ("tokyo-night", TOKYO_NIGHT),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#89b4fa"), Some(Color::from_u32(0x89b4fa)));
        assert_eq!(parse_hex("89b4fa"), Some(Color::from_u32(0x89b4fa)));
        assert_eq!(parse_hex("nope"), None);
    }

    #[test]
    fn overrides_merge_onto_mocha() {
        let toml = r##"
            [semantic]
            border_focused = "#ff0000"
            unknown_role = "#00ff00"
        "##;
        let overrides: ThemeOverrides = toml::from_str(toml).unwrap();
        let mut theme = Theme::catppuccin_mocha();
        overrides.apply(&mut theme);
        assert_eq!(theme.semantic.border_focused, Color::from_u32(0xff0000));
        // untouched roles keep mocha defaults
        assert_eq!(theme.semantic.directory, Color::from_u32(0x89b4fa));
    }

    #[test]
    fn embedded_palettes_all_load_and_differ() {
        let mocha = Theme::catppuccin_mocha();
        let mut bases = vec![mocha.semantic.base];
        for (name, roles) in EMBEDDED {
            let theme = Theme::embedded(name).expect("embedded theme loads");
            if roles.is_empty() {
                continue; // mocha baseline
            }
            assert_ne!(
                theme.semantic.base, mocha.semantic.base,
                "{name} should differ from mocha"
            );
            assert!(
                !bases.contains(&theme.semantic.base),
                "{name} base collides with another palette"
            );
            bases.push(theme.semantic.base);
        }
    }

    #[test]
    fn unknown_name_falls_back_to_mocha() {
        let theme = Theme::load("no-such-theme");
        assert_eq!(theme.semantic.base, Theme::catppuccin_mocha().semantic.base);
        assert!(Theme::embedded("no-such-theme").is_none());
    }

    #[test]
    fn available_names_lists_embedded() {
        let names = Theme::available_names();
        for expected in EMBEDDED.iter().map(|(n, _)| *n) {
            assert!(names.contains(&expected.to_string()), "{expected} missing");
        }
    }
}
