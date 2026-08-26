//! Bottom modeline: mode chip · forge chip · `>` caret · status ·
//! context · key hints (PLAN.md §2, §5). Everything is fitted to the
//! line width — hints drop whole from the tail (marked `…`), the
//! status is capped, the context truncates last.

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
    }
}

pub struct Modeline {
    /// Active provider identity ("github", "gitlab", config-supplied).
    pub forge: String,
    pub context: String,
    /// Transient one-line status ("searching…", errors) shown after
    /// the caret, in warning color.
    pub status: Option<String>,
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
            context: String::new(),
            status: None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, mode: Mode, theme: &Theme) {
        let sem = &theme.semantic;
        let w = area.width as usize;

        // Mode chip · forge chip · vim-style `>` caret.
        let mut spans = vec![
            Span::styled(
                format!(" {} ", mode.chip()),
                Style::default()
                    .fg(sem.crust)
                    .bg(mode_color(mode, sem))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", fit(&self.forge, 12)),
                Style::default().fg(sem.crust).bg(sem.forge),
            ),
            Span::styled(
                " > ",
                Style::default()
                    .fg(sem.forge)
                    .bg(sem.mantle)
                    .add_modifier(Modifier::BOLD),
            ),
        ];

        // Transient status, capped at half the line so it can't eat
        // the hints; middle-truncated (head + … + tail) so paths and
        // URLs keep their meaningful end.
        if let Some(status) = &self.status {
            let cap = (w / 2).max(20);
            spans.push(Span::styled(
                format!(" {} ", fit_middle(status, cap)),
                Style::default().fg(sem.warning).bg(sem.mantle),
            ));
        }
        let left_w: usize = spans.iter().map(|s| s.content.width()).sum();
        // The context reserves room first (capped at a quarter of the
        // line), then hints fit the remainder — dropping whole hints
        // from the tail, an ellipsis marking the cut.
        let ctx = &self.context;
        let ctx_w = UnicodeWidthStr::width(ctx.as_str());
        let reserved = (ctx_w + 2).min((w / 4).max(20));
        let mut hints: Vec<Span> = Vec::new();
        let mut hints_w = 0;
        let hint_room = w.saturating_sub(left_w + reserved);
        for (k, desc) in keymap::hints(mode) {
            let needed = UnicodeWidthStr::width(*k) + UnicodeWidthStr::width(*desc) + 4;
            if hints_w + needed <= hint_room {
                hints.push(Span::styled(
                    format!(" {k}"),
                    Style::default()
                        .fg(sem.text)
                        .bg(sem.mantle)
                        .add_modifier(Modifier::BOLD),
                ));
                hints.push(Span::styled(
                    format!(" {desc} ·"),
                    Style::default().fg(sem.hint).bg(sem.mantle),
                ));
                hints_w += needed;
            } else {
                if hint_room - hints_w >= 2 {
                    hints.push(Span::styled(
                        " …",
                        Style::default().fg(sem.hint).bg(sem.mantle),
                    ));
                    hints_w += 2;
                }
                break;
            }
        }

        // Context: whatever remains after the hints.
        let ctx_room = w.saturating_sub(left_w + hints_w);
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

        let pad =
            w.saturating_sub(spans.iter().map(|s| s.content.width()).sum::<usize>() + hints_w);
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().bg(sem.mantle),
        ));
        spans.extend(hints);

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

/// Middle truncation: keep the first third and the tail, `…` between —
/// the ends of a path or sentence carry the meaning.
fn fit_middle(s: &str, width: usize) -> String {
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
        let mut terminal = Terminal::new(TestBackend::new(w, 1)).unwrap();
        terminal
            .draw(|f| m.render(f, f.area(), mode, &crate::theme::Theme::catppuccin_mocha()))
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..w).map(|x| buf[(x, 0)].symbol().to_string()).collect()
    }

    fn sample() -> Modeline {
        Modeline {
            forge: "github".into(),
            context: "ratatui/ratatui · main".into(),
            status: None,
        }
    }

    #[test]
    fn wide_line_shows_forge_caret_context_and_all_hints() {
        let line = row(&sample(), Mode::Browse, 180);
        assert!(line.contains("BROWSE"));
        assert!(line.contains("github"));
        assert!(line.contains('>'), "vim-like caret: {line}");
        assert!(line.contains("ratatui/ratatui · main"));
        assert!(line.contains("q quit"), "last hint should survive: {line}");
        assert_eq!(line.chars().count(), 180);
    }

    #[test]
    fn narrow_line_drops_tail_hints_with_ellipsis() {
        let line = row(&sample(), Mode::Browse, 60);
        assert!(line.contains("…"), "cut should be marked: {line}");
        assert!(line.contains("j/k move"));
        assert!(!line.contains("q quit"));
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
