//! Unspecified diff roles follow the selected palette, including light
//! backgrounds. Explicit palette/user overrides always win.

use super::Semantic;
use ratatui::style::Color;

const ROW_TINT_PERCENT: u16 = 12;
const EMPHASIS_TINT_PERCENT: u16 = 30;
const PERCENT_SCALE: u16 = 100;

pub(super) fn apply(semantic: &mut Semantic, explicit: impl Fn(&str) -> bool) {
    if !explicit("diff_add_fg") {
        semantic.diff_add_fg = semantic.mode_browse;
    }
    if !explicit("diff_del_fg") {
        semantic.diff_del_fg = semantic.error;
    }
    if !explicit("diff_add_bg") {
        semantic.diff_add_bg = tint(semantic.base, semantic.diff_add_fg, ROW_TINT_PERCENT);
    }
    if !explicit("diff_del_bg") {
        semantic.diff_del_bg = tint(semantic.base, semantic.diff_del_fg, ROW_TINT_PERCENT);
    }
    if !explicit("diff_add_strong") {
        semantic.diff_add_strong = tint(semantic.base, semantic.diff_add_fg, EMPHASIS_TINT_PERCENT);
    }
    if !explicit("diff_del_strong") {
        semantic.diff_del_strong = tint(semantic.base, semantic.diff_del_fg, EMPHASIS_TINT_PERCENT);
    }
    if !explicit("diff_band") {
        semantic.diff_band = semantic.surface0;
    }
}

fn tint(base: Color, foreground: Color, percent: u16) -> Color {
    match (base, foreground) {
        (Color::Rgb(red, green, blue), Color::Rgb(tint_red, tint_green, tint_blue)) => {
            let mix = |base, tint| {
                ((u16::from(base) * (PERCENT_SCALE - percent) + u16::from(tint) * percent)
                    / PERCENT_SCALE) as u8
            };
            Color::Rgb(
                mix(red, tint_red),
                mix(green, tint_green),
                mix(blue, tint_blue),
            )
        }
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn light_palette_diffs_do_not_inherit_dark_mocha_bands() {
        let light = Theme::embedded("github-light").unwrap().semantic;
        let dark = Theme::catppuccin_mocha().semantic;
        let brightness = |color| match color {
            Color::Rgb(red, green, blue) => u16::from(red) + u16::from(green) + u16::from(blue),
            _ => 0,
        };
        assert!(brightness(light.diff_add_bg) > brightness(dark.diff_add_bg));
        assert!(brightness(light.diff_del_bg) > brightness(dark.diff_del_bg));
        assert_ne!(light.diff_add_bg, light.diff_add_strong);
    }
}
