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

    pub badge_repo: Color,
    pub badge_org: Color,
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

                badge_repo: Color::from_u32(0x89b4fa), // blue
                badge_org: Color::from_u32(0xfab387),  // peach
            },
        }
    }
}
