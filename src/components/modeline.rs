//! Bottom modeline, powerline style (omp/nvim-inspired): mode chip →
//! forge chip → context, joined by segment arrows that carry the left
//! segment's color into the right one's background. Nerd Font glyphs
//! (`[ui] nerd_font = true`) draw true powerline arrows and forge
//! icons; the default uses the starship-style `❯` and text-only
//! chips so non-Nerd-Font terminals never see tofu. Everything is
//! fitted to the line width — hints drop whole from the tail (marked
//! `…`), the status is capped, the context truncates last.

use super::pane::fit;
use crate::keymap;
use crate::mode::Mode;
use crate::theme::{Semantic, Theme};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

/// Chip color per mode — modeline, keybinds popup, anywhere a mode
/// chip is drawn.
pub(crate) fn mode_color(mode: Mode, sem: &Semantic) -> Color {
    match mode {
        Mode::Browse => sem.mode_browse,
        Mode::Search | Mode::Find => sem.mode_search,
        Mode::Insert => sem.mode_insert,
        Mode::Normal => sem.mode_normal,
        Mode::Leader => sem.mode_leader,
        Mode::Visual => sem.mode_visual,
        Mode::History => sem.mode_search,
        Mode::Preview => sem.mode_search,
    }
}

/// The icon spec a provider declares (handshake `icon`, protocol
/// v1.3 — or a `[provider] icon` config override): a builtin name
/// mapping to its Nerd Font glyph (rendered only with nerd_font on —
/// they're PUA), or a single literal glyph the terminal can render in
/// any mode. Rootle hardcodes nothing but its own in-tree github.
fn resolve_icon(spec: Option<&str>, nerd_font: bool) -> String {
    let Some(spec) = spec else {
        return String::new();
    };
    let named = match spec {
        "github" => Some("\u{f408}"),        // oct-mark_github
        "gitlab" => Some("\u{f296}"),        // fa-gitlab
        "bitbucket" => Some("\u{f171}"),     // fa-bitbucket
        "folder" | "fs" => Some("\u{f07c}"), // fa-folder_open
        _ => None,
    };
    if let (Some(glyph), true) = (named, nerd_font) {
        return glyph.into();
    }
    // A single scalar passes through verbatim — the provider vouched
    // for its font coverage.
    let mut chars = spec.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c.to_string(),
        _ => String::new(),
    }
}

pub struct Modeline {
    /// Active provider identity ("github", "gitlab", config-supplied).
    pub forge: String,
    /// Provider-declared icon spec (builtin name or literal glyph).
    pub icon: Option<String>,
    pub context: String,
    /// Transient one-line status ("searching/loading/error") shown after
    /// the caret, in warning color.
    pub status: Option<String>,
    /// A newer release tag when the startup check found one (0017 M3)
    /// — an accent ` ↑ vX.Y.Z ` before the keys affordance.
    pub update_tag: Option<String>,
}

impl Default for Modeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Modeline {
    pub fn new() -> Modeline {
        Modeline {
            forge: String::new(),
            icon: None,
            context: String::new(),
            status: None,
            update_tag: None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, mode: Mode, theme: &Theme) {
        let sem = &theme.semantic;
        let w = area.width as usize;
        let chip_bg = mode_color(mode, sem);

        // Mode chip → forge chip, joined by powerline bridges: the
        // arrow's fg is the segment it leaves, its bg the segment it
        // enters (the classic powerline gradient).
        // Powerline arrows with Nerd Font; otherwise the configured
        // separator — pipe (rectangular chips) by default, caret (❯)
        // on request. The ❯ default died: it read as broken without
        // Nerd Fonts.
        let arrow = if theme.nerd_font {
            "\u{e0b0}"
        } else {
            theme.separator.glyph()
        };
        let icon = resolve_icon(self.icon.as_deref(), theme.nerd_font);
        let forge_label = fit(&self.forge, 12);
        // Segment padding: a space on both sides of the content —
        // the powerline arrow must never touch the icon or the label.
        let forge_text = if icon.is_empty() {
            format!(" {forge_label} ")
        } else {
            format!(" {icon} {forge_label} ")
        };
        let mut spans = vec![
            Span::styled(
                format!(" {} ", mode.chip()),
                Style::default()
                    .fg(sem.crust)
                    .bg(chip_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(arrow, Style::default().fg(chip_bg).bg(sem.forge)),
            Span::styled(forge_text, Style::default().fg(sem.crust).bg(sem.forge)),
            Span::styled(arrow, Style::default().fg(sem.forge).bg(sem.mantle)),
        ];

        // Transient status, capped at half the line; middle-truncated
        // (head + … + tail) so paths and URLs keep their meaningful end.
        if let Some(status) = &self.status {
            let cap = (w / 2).max(20);
            spans.push(Span::styled(
                format!(" {} ", fit_middle(status, cap)),
                Style::default().fg(sem.warning).bg(sem.mantle),
            ));
        }
        let left_w: usize = spans.iter().map(|s| s.content.width()).sum();
        // State only — helix/kakoune rule: a statusline shows WHERE you
        // are, never a key catalog. Keys live in the mode strip above
        // (transient modes) and the `?` popup; the modeline keeps one
        // affordance pointing at them.
        let ctx = &self.context;
        let ctx_w = UnicodeWidthStr::width(ctx.as_str());
        let affordance = "? keys";
        let afford_w = UnicodeWidthStr::width(affordance) + 2;
        let ctx_room = w.saturating_sub(left_w + afford_w);
        if ctx_room >= ctx_w + 2 {
            spans.push(Span::styled(
                format!(" {ctx} "),
                Style::default().fg(sem.subtext0).bg(sem.mantle),
            ));
        } else if ctx_room >= 4 {
            spans.push(Span::styled(
                format!(" {} ", fit(ctx, ctx_room - 2)),
                Style::default().fg(sem.subtext0).bg(sem.mantle),
            ));
        }

        // The update chip (0017 M3): a newer release, accent — left of
        // the keys affordance.
        let chip = self.update_tag.as_ref().map(|t| format!(" ↑ {t} "));
        let chip_w = chip
            .as_ref()
            .map(|c| UnicodeWidthStr::width(c.as_str()))
            .unwrap_or(0);
        let pad = w
            .saturating_sub(spans.iter().map(|s| s.content.width()).sum::<usize>())
            .saturating_sub(afford_w + chip_w);
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().bg(sem.mantle),
        ));
        if let Some(chip) = chip
            && w >= left_w + afford_w + chip_w + 4
        {
            spans.push(Span::styled(
                chip,
                Style::default()
                    .fg(sem.warning)
                    .bg(sem.mantle)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if w >= left_w + afford_w + 4 {
            spans.push(Span::styled(
                format!(" {affordance} "),
                Style::default().fg(sem.hint).bg(sem.mantle),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

/// The mode hint strip — helix's glued space-menu flattened to one
/// line, rendered directly above the modeline in transient modes
/// (never in Browse: the catalog would just eat a content row).
/// Packs whole hints, drops from the tail with an ellipsis.
pub fn hint_strip_line(mode: Mode, width: usize, theme: &Theme) -> Line<'static> {
    let sem = &theme.semantic;
    let base = Style::default().bg(sem.mantle);
    let mut spans: Vec<Span> = vec![Span::styled(" ", base)];
    let mut used = 1;
    for (k, desc) in keymap::hints(mode) {
        let needed = UnicodeWidthStr::width(*k) + UnicodeWidthStr::width(*desc) + 5;
        if used + needed > width {
            if width - used >= 2 {
                spans.push(Span::styled(" …", base.fg(sem.hint)));
            }
            break;
        }
        spans.push(Span::styled(
            format!(" {k}"),
            base.fg(sem.text).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {desc} ·"), base.fg(sem.hint)));
        used += needed;
    }
    let pad = width.saturating_sub(spans.iter().map(|s| s.content.width()).sum::<usize>());
    spans.push(Span::styled(" ".repeat(pad), base));
    Line::from(spans)
}

/// Middle truncation: keep the first third and the tail, `…` between —
/// the ends of a path or sentence carry the meaning.
pub(crate) fn fit_middle(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    let head_room = width / 3;
    let tail_room = width.saturating_sub(head_room + 1);
    let mut head = String::new();
    let mut head_w = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if head_w + cw > head_room {
            break;
        }
        head_w += cw;
        head.push(c);
    }
    let mut tail = String::new();
    let mut tail_w = 0;
    for c in s.chars().rev() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if tail_w + cw > tail_room {
            break;
        }
        tail_w += cw;
        tail.insert(0, c);
    }
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn row(m: &Modeline, mode: Mode, w: u16) -> String {
        row_with(m, mode, w, &crate::theme::Theme::catppuccin_mocha())
    }

    fn row_with(m: &Modeline, mode: Mode, w: u16, theme: &crate::theme::Theme) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, 1)).unwrap();
        terminal
            .draw(|f| m.render(f, f.area(), mode, theme))
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..w).map(|x| buf[(x, 0)].symbol().to_string()).collect()
    }

    fn sample() -> Modeline {
        Modeline {
            forge: "github".into(),
            icon: Some("github".into()),
            context: "ratatui/ratatui · main".into(),
            status: None,
            update_tag: None,
        }
    }

    #[test]
    fn nerd_font_swaps_arrows_and_adds_the_forge_icon() {
        let theme = crate::theme::Theme::catppuccin_mocha().with_nerd_font(true);
        let line = row_with(&sample(), Mode::Browse, 180, &theme);
        assert!(line.contains('\u{e0b0}'), "powerline arrow: {line}");
        assert!(line.contains('\u{f408}'), "github icon: {line}");
        assert!(
            !line.contains('❯'),
            "fallback caret must not appear: {line}"
        );
        assert_eq!(line.chars().count(), 180, "must not overflow: {line}");
    }

    #[test]
    fn no_declared_icon_renders_text_only_even_with_nerd_font() {
        let theme = crate::theme::Theme::catppuccin_mocha().with_nerd_font(true);
        let m = Modeline {
            forge: "internal".into(),
            icon: None,
            ..sample()
        };
        let line = row_with(&m, Mode::Browse, 120, &theme);
        assert!(line.contains("internal"));
        assert!(
            !line.contains('\u{f408}'),
            "rootle guesses no icons: {line}"
        );
    }

    #[test]
    fn literal_glyph_icon_renders_in_both_modes() {
        for nerd in [false, true] {
            let theme = crate::theme::Theme::catppuccin_mocha().with_nerd_font(nerd);
            let m = Modeline {
                icon: Some("◆".into()),
                ..sample()
            };
            let line = row_with(&m, Mode::Browse, 120, &theme);
            assert!(line.contains('◆'), "literal glyph, nerd={nerd}: {line}");
        }
    }

    #[test]
    fn builtin_name_needs_nerd_font() {
        let plain = crate::theme::Theme::catppuccin_mocha();
        let line = row_with(&sample(), Mode::Browse, 120, &plain);
        assert!(
            !line.contains('\u{f408}'),
            "PUA glyph must not render without nerd_font: {line}"
        );
    }

    #[test]
    fn wide_line_shows_state_and_the_keys_affordance_no_catalog() {
        let line = row(&sample(), Mode::Browse, 180);
        assert!(line.contains("BROWSE"));
        assert!(line.contains("github"));
        assert!(line.contains("|"), "pipe separator by default: {line}");
        let caret_theme = crate::theme::Theme::catppuccin_mocha().with_separator("caret");
        let line = row_with(&sample(), Mode::Browse, 180, &caret_theme);
        assert!(line.contains('❯'), "caret on request: {line}");
        assert!(line.contains("ratatui/ratatui · main"));
        assert!(line.contains("? keys"), "the on-ramp affordance: {line}");
        assert!(
            !line.contains("q quit"),
            "the modeline carries no key catalog anymore: {line}"
        );
        assert_eq!(line.chars().count(), 180);
    }

    #[test]
    fn hint_strip_packs_and_marks_the_cut() {
        let theme = crate::theme::Theme::catppuccin_mocha();
        let backend = TestBackend::new(52, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                let line = hint_strip_line(Mode::Browse, area.width as usize, &theme);
                f.render_widget(ratatui::widgets::Paragraph::new(line), area);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let line: String = (0..52).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(line.contains("j/k move"), "first hints survive: {line}");
        assert!(line.contains("…"), "the cut is marked: {line}");
        assert!(!line.contains("q quit"), "tail dropped: {line}");
        assert_eq!(line.chars().count(), 52, "no overflow: {line}");
    }

    #[test]
    fn narrow_line_keeps_state_drops_context_before_the_affordance() {
        let line = row(&sample(), Mode::Browse, 60);
        assert!(line.contains("BROWSE"));
        assert!(line.contains("? keys"), "the affordance survives: {line}");
        assert!(!line.contains("j/k move"), "no inline catalog: {line}");
        assert_eq!(line.chars().count(), 60, "must not overflow: {line}");
    }

    #[test]
    fn tiny_line_keeps_chips_drops_everything_else() {
        let line = row(&sample(), Mode::Browse, 14);
        assert!(line.contains("BROWSE"));
        assert!(!line.contains("j/k"));
        assert_eq!(line.chars().count(), 14);
    }

    #[test]
    fn long_status_is_capped() {
        let m = Modeline {
            status: Some("provider stdio failed (spawn nosuchbinary: No such file or directory (os error 2))".into()),
            ..sample()
        };
        let line = row(&m, Mode::Browse, 90);
        assert!(
            !line.contains("No such file"),
            "middle must be dropped: {line}"
        );
        assert!(line.contains("provider stdio"), "head kept: {line}");
        assert!(line.contains("os error 2"), "tail kept: {line}");
        assert!(line.contains("…"));
    }
}
