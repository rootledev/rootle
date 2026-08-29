//! Embedded palettes as data (moved from theme.rs, plans/0021 M2
//! — a pure move): eleven themes as role→hex tables; `embedded()`
//! overlays them on the catppuccin-mocha baseline.

type RoleValue = (&'static str, u32);

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
    ("forge", 0x6272a4),       // comment
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
    ("forge", 0x5c6370),       // mono-3
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
    ("forge", 0x928374),       // gray
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
    ("forge", 0x616e88),       // polar night brightened
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
    ("forge", 0x565f89),       // comment
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
    ("forge", 0x657b83),       // base00
    ("badge_repo", 0x268bd2),
    ("badge_org", 0xcb4b16),
    ("search_match", 0xb58900),
];

// Light palettes. crust stays the text-on-accent-chip color — for
// light themes that is the palette's lightest tone (chips are
// saturated in every palette).

const CATPPUCCIN_LATTE: &[RoleValue] = &[
    ("crust", 0xdce0e8),
    ("mantle", 0xe6e9ef),
    ("base", 0xeff1f5),
    ("surface0", 0xccd0da),
    ("surface2", 0xacb0be),
    ("overlay0", 0x9ca0b0),
    ("subtext0", 0x6c6f85),
    ("text", 0x4c4f69),
    ("border_focused", 0x1e66f5), // blue
    ("border_unfocused", 0xacb0be),
    ("directory", 0x1e66f5),
    ("file", 0x4c4f69),
    ("selection_bg", 0xccd0da),
    ("selection_fg", 0x1e66f5),
    ("hint", 0x6c6f85),
    ("error", 0xd20f39),
    ("warning", 0xdf8e1d),     // yellow
    ("mode_browse", 0x40a02b), // green
    ("mode_search", 0xdf8e1d),
    ("mode_insert", 0x179299), // teal
    ("mode_normal", 0x1e66f5),
    ("mode_leader", 0xfe640b), // peach
    ("mode_visual", 0xea76cb), // pink
    ("forge", 0x9ca0b0),       // overlay0
    ("badge_repo", 0x1e66f5),
    ("badge_org", 0xfe640b),
    ("search_match", 0xdf8e1d),
];

const GITHUB_LIGHT: &[RoleValue] = &[
    ("crust", 0xffffff),
    ("mantle", 0xf6f8fa),
    ("base", 0xffffff),
    ("surface0", 0xeaeef2),
    ("surface2", 0xd1d9e0),
    ("overlay0", 0x818b98),
    ("subtext0", 0x59636e),
    ("text", 0x1f2328),
    ("border_focused", 0x0969da), // accent blue
    ("border_unfocused", 0xd1d9e0),
    ("directory", 0x0969da),
    ("file", 0x1f2328),
    ("selection_bg", 0xddf4ff), // accent subtle
    ("selection_fg", 0x0969da),
    ("hint", 0x59636e),
    ("error", 0xd1242f),
    ("warning", 0x9a6700),     // yellow
    ("mode_browse", 0x1a7f37), // green
    ("mode_search", 0x9a6700),
    ("mode_insert", 0x0a7ea4), // teal-ish
    ("mode_normal", 0x0969da),
    ("mode_leader", 0xbc4c00), // orange
    ("mode_visual", 0x8250df), // purple
    ("forge", 0x818b98),       // primer gray
    ("badge_repo", 0x0969da),
    ("badge_org", 0xbc4c00),
    ("search_match", 0x9a6700),
];

const ONE_LIGHT: &[RoleValue] = &[
    ("crust", 0xfafafa),
    ("mantle", 0xf0f0f0),
    ("base", 0xfafafa),
    ("surface0", 0xeaeaeb),
    ("surface2", 0xcaccd1),
    ("overlay0", 0xa0a1a7),
    ("subtext0", 0x696c77),
    ("text", 0x383a42),
    ("border_focused", 0x4078f2), // blue
    ("border_unfocused", 0xa0a1a7),
    ("directory", 0x4078f2),
    ("file", 0x383a42),
    ("selection_bg", 0xeaeaeb),
    ("selection_fg", 0x4078f2),
    ("hint", 0x696c77),
    ("error", 0xe45649),
    ("warning", 0xc18401),     // yellow
    ("mode_browse", 0x50a14f), // green
    ("mode_search", 0xc18401),
    ("mode_insert", 0x0184bc), // cyan
    ("mode_normal", 0x4078f2),
    ("mode_leader", 0xb76b01), // dark orange
    ("mode_visual", 0xa626a4), // purple
    ("forge", 0xa0a1a7),       // mono-3
    ("badge_repo", 0x4078f2),
    ("badge_org", 0xb76b01),
    ("search_match", 0xc18401),
];

const SOLARIZED_LIGHT: &[RoleValue] = &[
    ("crust", 0xfdf6e3),  // base3 — lightest, text on accent chips
    ("mantle", 0xeee8d5), // base2
    ("base", 0xfdf6e3),   // base3
    ("surface0", 0xeee8d5),
    ("surface2", 0x93a1a1),       // base1 — visible unfocused border
    ("overlay0", 0x839496),       // base0
    ("subtext0", 0x657b83),       // base00
    ("text", 0x586e75),           // base01
    ("border_focused", 0x268bd2), // blue
    ("border_unfocused", 0x93a1a1),
    ("directory", 0x268bd2),
    ("file", 0x586e75),
    ("selection_bg", 0xeee8d5),
    ("selection_fg", 0x268bd2),
    ("hint", 0x657b83),
    ("error", 0xdc322f),
    ("warning", 0xb58900),     // yellow
    ("mode_browse", 0x859900), // green
    ("mode_search", 0xb58900),
    ("mode_insert", 0x2aa198), // cyan
    ("mode_normal", 0x268bd2),
    ("mode_leader", 0xcb4b16), // orange
    ("mode_visual", 0x6c71c4), // violet
    ("forge", 0x839496),       // base0
    ("badge_repo", 0x268bd2),
    ("badge_org", 0xcb4b16),
    ("search_match", 0xb58900),
];

// ---------------------------------------------------------------------------
// Syntax tables — one per palette, values from each published spec
// (Dracula spec, Atom One Dark/Light, gruvbox, nord, tokyo-night,
// solarized, Catppuccin Latte, GitHub Primer). Limited-palette schemes
// (gruvbox, solarized, nord) reuse hues across roles by design.
// ---------------------------------------------------------------------------

const DRACULA_SYNTAX: &[RoleValue] = &[
    ("keyword", 0xff79c6),   // pink
    ("string", 0xf1fa8c),    // yellow
    ("comment", 0x6272a4),   // comment blue-gray
    ("function", 0x50fa7b),  // green
    ("type", 0x8be9fd),      // cyan
    ("constant", 0xbd93f9),  // purple
    ("tag", 0xff79c6),       // pink
    ("namespace", 0x8be9fd), // cyan
    ("invalid", 0xff5555),   // red
];

const ONE_DARK_SYNTAX: &[RoleValue] = &[
    ("keyword", 0xc678dd),   // purple (hue-4)
    ("string", 0x98c379),    // green (hue-2)
    ("comment", 0x5c6370),   // mono-3
    ("function", 0x61afef),  // blue (hue-1)
    ("type", 0xe5c07b),      // yellow (hue-6-2)
    ("constant", 0xd19a66),  // orange (hue-6)
    ("tag", 0xe06c75),       // red (hue-5)
    ("namespace", 0x56b6c2), // cyan (hue-3)
    ("invalid", 0xe06c75),
];

const GRUVBOX_DARK_SYNTAX: &[RoleValue] = &[
    ("keyword", 0xfb4934),   // red
    ("string", 0xb8bb26),    // green
    ("comment", 0x928374),   // gray
    ("function", 0xb8bb26),  // green (gruvbox Function)
    ("type", 0xfabd2f),      // yellow
    ("constant", 0xd3869b),  // purple
    ("tag", 0xfb4934),       // red
    ("namespace", 0x8ec07c), // aqua
    ("invalid", 0xfb4934),
];

const NORD_SYNTAX: &[RoleValue] = &[
    ("keyword", 0x81a1c1),   // frost nord9
    ("string", 0xa3be8c),    // aurora green
    ("comment", 0x616e88),   // polar night brightened
    ("function", 0x88c0d0),  // frost nord8
    ("type", 0x8fbcbb),      // frost nord7
    ("constant", 0xb48ead),  // aurora purple
    ("tag", 0x81a1c1),       // frost nord9
    ("namespace", 0x8fbcbb), // frost nord7
    ("invalid", 0xbf616a),   // aurora red
];

const TOKYO_NIGHT_SYNTAX: &[RoleValue] = &[
    ("keyword", 0xbb9af7),   // purple
    ("string", 0x9ece6a),    // green
    ("comment", 0x565f89),   // comment
    ("function", 0x7aa2f7),  // blue
    ("type", 0x7dcfff),      // cyan
    ("constant", 0xff9e64),  // orange
    ("tag", 0xf7768e),       // red
    ("namespace", 0x7dcfff), // cyan
    ("invalid", 0xf7768e),
];

const SOLARIZED_SYNTAX: &[RoleValue] = &[
    // Accent hues are shared between solarized dark and light; the
    // semantic tables carry the polarity difference.
    ("keyword", 0x859900),   // green
    ("string", 0x2aa198),    // cyan
    ("comment", 0x586e75),   // base01 (dark) — light overrides below
    ("function", 0x268bd2),  // blue
    ("type", 0xb58900),      // yellow
    ("constant", 0x2aa198),  // cyan (solarized Constant)
    ("tag", 0x268bd2),       // blue
    ("namespace", 0x6c71c4), // violet
    ("invalid", 0xdc322f),   // red
];

const SOLARIZED_LIGHT_SYNTAX: &[RoleValue] = &[
    ("keyword", 0x859900),
    ("string", 0x2aa198),
    ("comment", 0x93a1a1), // base1 — the light polarity's muted tone
    ("function", 0x268bd2),
    ("type", 0xb58900),
    ("constant", 0x2aa198),
    ("tag", 0x268bd2),
    ("namespace", 0x6c71c4),
    ("invalid", 0xdc322f),
];

const CATPPUCCIN_LATTE_SYNTAX: &[RoleValue] = &[
    ("keyword", 0x8839ef),   // mauve
    ("string", 0x40a02b),    // green
    ("comment", 0x9ca0b0),   // overlay0
    ("function", 0x1e66f5),  // blue
    ("type", 0xdf8e1d),      // yellow
    ("constant", 0xfe640b),  // peach
    ("tag", 0xd20f39),       // red
    ("namespace", 0x179299), // teal
    ("invalid", 0xd20f39),
];

const GITHUB_LIGHT_SYNTAX: &[RoleValue] = &[
    // Primer prettylights (github.com/primer/primitives).
    ("keyword", 0xcf222e),   // prettylights syntax keyword
    ("string", 0x0a3069),    // string
    ("comment", 0x6e7781),   // comment
    ("function", 0x8250df),  // entity (function)
    ("type", 0x0550ae),      // constant / class
    ("constant", 0x0550ae),  // constant
    ("tag", 0x116329),       // tag
    ("namespace", 0x953800), // variable-ish orange
    ("invalid", 0xcf222e),
];

const ONE_LIGHT_SYNTAX: &[RoleValue] = &[
    ("keyword", 0xa626a4),   // purple
    ("string", 0x50a14f),    // green
    ("comment", 0xa0a1a7),   // mono-3
    ("function", 0x4078f2),  // blue
    ("type", 0xc18401),      // yellow
    ("constant", 0xb76b01),  // orange
    ("tag", 0xe45649),       // red
    ("namespace", 0x0184bc), // cyan
    ("invalid", 0xe45649),
];

pub(crate) const EMBEDDED: &[(&str, &[RoleValue], &[RoleValue])] = &[
    ("catppuccin-mocha", &[], &[]), // baseline, constructed directly
    ("dracula", DRACULA, DRACULA_SYNTAX),
    ("gruvbox-dark", GRUVBOX_DARK, GRUVBOX_DARK_SYNTAX),
    ("nord", NORD, NORD_SYNTAX),
    ("one-dark", ONE_DARK, ONE_DARK_SYNTAX),
    ("solarized-dark", SOLARIZED_DARK, SOLARIZED_SYNTAX),
    ("tokyo-night", TOKYO_NIGHT, TOKYO_NIGHT_SYNTAX),
    // light
    (
        "catppuccin-latte",
        CATPPUCCIN_LATTE,
        CATPPUCCIN_LATTE_SYNTAX,
    ),
    ("github-light", GITHUB_LIGHT, GITHUB_LIGHT_SYNTAX),
    ("one-light", ONE_LIGHT, ONE_LIGHT_SYNTAX),
    ("solarized-light", SOLARIZED_LIGHT, SOLARIZED_LIGHT_SYNTAX),
];
