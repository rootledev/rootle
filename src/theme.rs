//! Catppuccin Mocha palette + semantic roles.
//! External palette files (PLAN.md §4) land with the theme loader in a
//! later milestone; the embedded Mocha default is the fallback forever.

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

    /// Every theme name the loader can resolve: the embedded default
    /// plus any `themes/<name>.toml` in the config dir. Settings list.
    pub fn available_names() -> Vec<String> {
        let mut names = vec!["catppuccin-mocha".to_string()];
        if let Some(dir) = dirs::config_dir().map(|d| d.join("ghx").join("themes"))
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

    /// Load the named theme: embedded catppuccin-mocha defaults, merged
    /// with `~/.config/ghx/themes/<name>.toml` overrides (missing file,
    /// malformed TOML, unknown roles, bad hex → silently keep defaults;
    /// theming must never crash the app).
    pub fn load(name: &str) -> Self {
        let mut theme = Self::catppuccin_mocha();
        let Some(dir) = dirs::config_dir() else {
            return theme;
        };
        let path = dir.join("ghx").join("themes").join(format!("{name}.toml"));
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

impl ThemeOverrides {
    fn apply(self, theme: &mut Theme) {
        let sem = &mut theme.semantic;
        for (role, hex) in self.semantic {
            let Some(color) = parse_hex(&hex) else {
                continue;
            };
            match role.as_str() {
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
    }
}

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
}
