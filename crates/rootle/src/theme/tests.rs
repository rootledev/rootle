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
fn unknown_name_falls_back_to_mocha() {
    let theme = Theme::load("no-such-theme");
    assert_eq!(theme.semantic.base, Theme::catppuccin_mocha().semantic.base);
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
        assert_eq!(syntax.len(), 9, "{name} syntax table incomplete");
        assert_eq!(roles.len(), 27, "{name} semantic table incomplete");
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
