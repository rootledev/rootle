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
    for (name, roles, _) in palettes::EMBEDDED {
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
fn unknown_name_falls_back_to_hone() {
    let theme = Theme::load("no-such-theme");
    assert_eq!(theme.semantic.base, Theme::hone().semantic.base);
    assert!(Theme::embedded("no-such-theme").is_none());
}

#[test]
fn syntax_overrides_merge_onto_base() {
    let toml = r##"
        [semantic]
        border_focused = "#ff0000"
        [syntax]
        keyword = "#123456"
        type = "#654321"
        bogus = "#000000"
    "##;
    let overrides: ThemeOverrides = toml::from_str(toml).unwrap();
    let mut theme = Theme::catppuccin_mocha();
    overrides.apply(&mut theme);
    assert_eq!(theme.syntax.keyword, Color::from_u32(0x123456));
    assert_eq!(theme.syntax.type_, Color::from_u32(0x654321));
    // untouched syntax roles keep mocha defaults
    assert_eq!(theme.syntax.string, Color::from_u32(0xa6e3a1));
    // semantic overrides still apply alongside
    assert_eq!(theme.semantic.border_focused, Color::from_u32(0xff0000));
}

#[test]
fn embedded_palettes_have_spec_syntax() {
    let mocha = Theme::catppuccin_mocha().syntax;
    for (name, roles, syntax) in palettes::EMBEDDED {
        let theme = Theme::embedded(name).expect("embedded theme loads");
        if syntax.is_empty() {
            continue; // mocha baseline
        }
        // Every palette table is complete — no silent mocha holes.
        let expected = if theme.syntax.number.is_some() { 18 } else { 9 };
        assert_eq!(syntax.len(), expected, "{name} syntax table incomplete");
        assert!(roles.len() >= 27, "{name} semantic table incomplete");
        assert_ne!(
            theme.syntax, mocha,
            "{name} syntax should differ from mocha"
        );
    }
    // Spot-check spec values (dracula keyword is pink, github-light
    // keyword is primer red).
    let dracula = Theme::embedded("dracula").unwrap();
    assert_eq!(dracula.syntax.keyword, Color::from_u32(0xff79c6));
    let gh = Theme::embedded("github-light").unwrap();
    assert_eq!(gh.syntax.keyword, Color::from_u32(0xcf222e));
}

#[test]
fn border_shape_parses_and_defaults_plain() {
    assert_eq!(BorderShape::parse("rounded"), Some(BorderShape::Rounded));
    assert_eq!(BorderShape::parse(" Thick "), Some(BorderShape::Thick));
    assert_eq!(BorderShape::parse("double"), Some(BorderShape::Double));
    assert_eq!(BorderShape::parse("plain"), Some(BorderShape::Plain));
    assert_eq!(BorderShape::parse("squiggly"), None);
    let mocha = Theme::catppuccin_mocha();
    assert_eq!(mocha.border, BorderShape::Plain);
    assert_eq!(mocha.border_type(), BorderType::Plain);
    assert_eq!(
        mocha.with_border(BorderShape::Rounded).border_type(),
        BorderType::Rounded
    );
}
#[test]

fn available_names_lists_embedded() {
    let names = Theme::available_names();
    for expected in palettes::EMBEDDED.iter().map(|(n, _, _)| *n) {
        assert!(names.contains(&expected.to_string()), "{expected} missing");
    }
}

#[test]
fn fresh_profiles_use_hone_but_explicit_preferences_survive() {
    assert_eq!(crate::config::Config::default().theme.name, "hone");
    let config: crate::config::Config =
        toml::from_str("[theme]\nname = 'catppuccin-mocha'").unwrap();
    assert_eq!(config.theme.name, "catppuccin-mocha");
    assert_eq!(
        Theme::embedded(&config.theme.name).unwrap().semantic.base,
        Theme::catppuccin_mocha().semantic.base
    );
}

#[test]
fn strop_palettes_preserve_planes_modes_and_explicit_diff_washes() {
    for (name, base, surface, selection, add, strong) in [
        ("hone", 0x15171c, 0x1c1f25, 0x30405a, 0x1e2d22, 0x2b4a33),
        ("slate", 0x1a1c20, 0x202329, 0x343a45, 0x22302a, 0x2c4236),
        ("carbide", 0x0b0c0f, 0x14161b, 0x2f4a6e, 0x143d1f, 0x1e5a2e),
        ("vellum", 0xf6f2e9, 0xfdfbf6, 0xd1c9b3, 0xdfeadb, 0xc3dbbb),
        ("paper", 0xf8f8f3, 0xeef1e8, 0xc2d2ba, 0xe3ecdc, 0xcbdcbf),
        (
            "paper-dark",
            0x151d19,
            0x1c2720,
            0x2f4b3a,
            0x203b2b,
            0x304f38,
        ),
    ] {
        let sem = Theme::embedded(name).unwrap().semantic;
        assert_eq!(sem.base, Color::from_u32(base), "{name}");
        assert_eq!(sem.mantle, Color::from_u32(surface), "{name}");
        assert_eq!(sem.selection_bg, Color::from_u32(selection), "{name}");
        assert_eq!(sem.diff_add_bg, Color::from_u32(add), "{name}");
        assert_eq!(sem.diff_add_strong, Color::from_u32(strong), "{name}");
        assert_eq!(sem.selection_fg, sem.text);
        assert_eq!(sem.mode_browse, sem.mode_normal);
        assert_ne!(sem.mode_insert, sem.mode_normal);
        assert_ne!(sem.mode_visual, sem.mode_insert);
        assert_eq!(sem.forge, sem.surface0);
        assert_eq!(sem.forge_fg, sem.subtext0);
    }
}

#[test]
fn hone_overrides_keep_unmodified_tokens_and_accept_provider_ink() {
    let mut theme = Theme::hone();
    let original = theme;
    let overrides: ThemeOverrides = toml::from_str(
        "[semantic]\nforge_fg = '#123456'\nselection_bg = '#234567'\n\
         diff_add_strong = '#345678'\n[syntax]\nkeyword = '#456789'",
    )
    .unwrap();
    overrides.apply(&mut theme);
    assert_eq!(theme.semantic.forge_fg, Color::from_u32(0x123456));
    assert_eq!(theme.semantic.selection_bg, Color::from_u32(0x234567));
    assert_eq!(theme.semantic.diff_add_strong, Color::from_u32(0x345678));
    assert_eq!(theme.syntax.keyword, Color::from_u32(0x456789));
    assert_eq!(theme.semantic.diff_add_bg, original.semantic.diff_add_bg);
    assert_eq!(theme.syntax.string, original.syntax.string);
}

fn luminance(color: Color) -> f64 {
    let Color::Rgb(r, g, b) = color else {
        panic!("embedded palettes must use RGB");
    };
    let linear = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn strop_text_selection_chips_and_syntax_keep_readable_contrast() {
    for name in ["hone", "slate", "carbide", "vellum", "paper", "paper-dark"] {
        let theme = Theme::embedded(name).unwrap();
        let sem = theme.semantic;
        assert!(contrast(sem.text, sem.base) >= 10.0, "{name} text");
        assert!(
            contrast(sem.selection_fg, sem.selection_bg) >= 4.5,
            "{name} selection"
        );
        assert!(
            contrast(sem.crust, sem.search_match) >= 4.5,
            "{name} search"
        );
        assert!(contrast(sem.forge_fg, sem.forge) >= 4.5, "{name} provider");
        for ink in [
            sem.mode_normal,
            sem.mode_insert,
            sem.mode_visual,
            sem.mode_search,
        ] {
            assert!(contrast(sem.crust, ink) >= 4.5, "{name} mode");
        }
        for (_, hex) in palettes::EMBEDDED
            .iter()
            .find(|(n, _, _)| *n == name)
            .unwrap()
            .2
        {
            let ink = Color::from_u32(*hex);
            assert!(contrast(ink, sem.base) >= 4.5, "{name} syntax/base");
            assert!(contrast(ink, sem.mantle) >= 4.5, "{name} syntax/surface");
        }
    }
}

#[test]
fn changing_hone_background_does_not_turn_additions_into_mode_amber() {
    let mut theme = Theme::hone();
    let add = theme.semantic.diff_add_fg;
    let overrides: ThemeOverrides = toml::from_str("[semantic]\nbase = '#202020'").unwrap();
    overrides.apply(&mut theme);
    assert_eq!(theme.semantic.diff_add_fg, add);
    assert_ne!(theme.semantic.diff_add_fg, theme.semantic.mode_browse);
    assert_ne!(
        theme.semantic.diff_add_bg,
        Theme::hone().semantic.diff_add_bg
    );
}
