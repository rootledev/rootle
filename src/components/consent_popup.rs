//! Declared-provider consent (plans/0019 M2): the config names a
//! provider this machine doesn't have — rootle asks before it
//! downloads and runs anything. `y` accepts the verified install
//! (status-line progress, then a hot-swap); `n`/Esc declines into
//! honest degraded mode. The declaration stays visible either way.

use crate::action::{Action, DeclarationState};
use crate::components::centered;
use crate::provider::Declaration;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// What the popup is asking about (0022 M2): install a missing
/// declared provider, or repair one that fails to start.
pub enum ConsentKind {
    Install(Declaration),
    Health(crate::provider::HealthIssue),
}

pub struct ConsentPopup {
    kind: ConsentKind,
    /// None = asking; Some(Installing) = worker running;
    /// Some(Failed) = refused, Esc closes.
    state: Option<DeclarationState>,
}

impl ConsentPopup {
    pub fn new(decl: Declaration) -> Self {
        ConsentPopup {
            kind: ConsentKind::Install(decl),
            state: None,
        }
    }

    /// The health variant (0022 M2): a provider that won't start.
    pub fn health(issue: crate::provider::HealthIssue) -> Self {
        ConsentPopup {
            kind: ConsentKind::Health(issue),
            state: None,
        }
    }

    pub fn declaration(&self) -> &Declaration {
        match &self.kind {
            ConsentKind::Install(d) => d,
            ConsentKind::Health(_) => unreachable!("health popup has no declaration"),
        }
    }

    pub fn health_issue(&self) -> Option<&crate::provider::HealthIssue> {
        match &self.kind {
            ConsentKind::Health(h) => Some(h),
            ConsentKind::Install(_) => None,
        }
    }

    /// Worker state landing (via `Action::DeclarationState`).
    pub fn set_state(&mut self, state: DeclarationState) {
        self.state = Some(state);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let retryable = self.health_issue().is_some_and(|h| h.retryable);
        match (&self.kind, key.code, &self.state) {
            (ConsentKind::Install(_), KeyCode::Char('y'), None) => Action::DeclarationAccept,
            (ConsentKind::Install(_), KeyCode::Char('n') | KeyCode::Esc, _) => {
                Action::DeclarationDecline
            }
            // 0022 M2 health: r retries (when sensible), g degrades to
            // github, e opens the config in the editor.
            (ConsentKind::Health(_), KeyCode::Char('r'), _) if retryable => {
                Action::DeclarationRetry
            }
            (ConsentKind::Health(_), KeyCode::Char('g') | KeyCode::Esc, _) => {
                Action::DeclarationDecline
            }
            (ConsentKind::Health(_), KeyCode::Char('e'), _) => Action::DeclarationEditConfig,
            _ => Action::Noop,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let area = centered(area, 64, 50);
        let mut lines = match &self.kind {
            ConsentKind::Health(issue) => health_lines(issue, sem),
            ConsentKind::Install(decl) => install_lines(decl, sem),
        };
        match &self.state {
            None => {}
            Some(DeclarationState::Installing) => lines.push(Line::from(Span::styled(
                "installing — verified download running…",
                Style::default().fg(sem.hint),
            ))),
            Some(DeclarationState::Failed(e)) => {
                lines.push(Line::from(Span::styled(
                    format!("failed: {e}"),
                    Style::default().fg(sem.error),
                )));
                lines.push(Line::from(Span::styled(
                    "esc — browse github (the declaration stays in your config)",
                    Style::default().fg(sem.subtext0),
                )));
            }
        }
        let title = match &self.kind {
            ConsentKind::Health(_) => " provider health ",
            ConsentKind::Install(_) => " install provider? ",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .title(Span::styled(
                title,
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ));
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(ratatui::widgets::Wrap { trim: false }),
            area,
        );
    }
}

/// Install-variant rows (moved verbatim from the single-kind popup).
fn install_lines(decl: &Declaration, sem: &crate::theme::Semantic) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("config declares ", Style::default().fg(sem.subtext0)),
            Span::styled(
                decl.name.clone(),
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — not installed on this machine",
                Style::default().fg(sem.subtext0),
            ),
        ]),
        Line::from(vec![
            Span::styled("source  ", Style::default().fg(sem.subtext0)),
            Span::styled(decl.repo.clone(), Style::default().fg(sem.text)),
        ]),
    ];
    if decl.tag.is_some() || decl.sha.is_some() {
        let pins = [
            decl.tag.clone().map(|t| format!("tag {t}")),
            decl.sha.is_some().then(|| "sha256".to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        lines.push(Line::from(vec![
            Span::styled("pins    ", Style::default().fg(sem.subtext0)),
            Span::styled(pins, Style::default().fg(sem.warning)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!(
            "you are trusting {} — the tarball is sha256-verified{} before anything runs",
            decl.repo,
            if decl.sha.is_some() {
                " against your config's pin and its sidecar"
            } else {
                " against its release sidecar"
            }
        ),
        Style::default().fg(sem.hint),
    )));
    lines.push(Line::from(vec![
        Span::styled(
            "y",
            Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" install  ·  ", Style::default().fg(sem.subtext0)),
        Span::styled(
            "n",
            Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" browse github instead", Style::default().fg(sem.subtext0)),
    ]));
    lines
}

/// Health-variant rows (0022 M2): name the provider, show the error,
/// offer the three choices.
fn health_lines(
    issue: &crate::provider::HealthIssue,
    sem: &crate::theme::Semantic,
) -> Vec<Line<'static>> {
    let choices = if issue.retryable {
        "r retry once  ·  g browse github  ·  e edit config"
    } else {
        "g browse github  ·  e edit config"
    };
    vec![
        Line::from(vec![
            Span::styled(
                issue.name.clone(),
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" failed to start", Style::default().fg(sem.error)),
        ]),
        Line::from(vec![
            Span::styled("error   ", Style::default().fg(sem.subtext0)),
            Span::styled(issue.error.clone(), Style::default().fg(sem.text)),
        ]),
        Line::raw(""),
        Line::from(Span::styled(choices, Style::default().fg(sem.subtext0))),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    fn decl() -> Declaration {
        Declaration {
            name: "gitlab".into(),
            repo: "rootledev/rootle-gitlab".into(),
            tag: None,
            sha: None,
        }
    }

    fn screen(popup: &mut ConsentPopup) -> String {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| popup.render(f, f.area(), &Theme::catppuccin_mocha()))
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, ratatui::crossterm::event::KeyModifiers::NONE)
    }

    /// 0019 M2: the popup asks — declaration, trust line, both keys
    /// named; y accepts, n declines, nothing else leaks out.
    #[test]
    fn asks_with_trust_line_and_maps_keys() {
        let mut popup = ConsentPopup::new(decl());
        let s = screen(&mut popup);
        assert!(s.contains("config declares gitlab"), "{s}");
        assert!(s.contains("rootledev/rootle-gitlab"), "{s}");
        assert!(
            s.contains("you are trusting rootledev/rootle-gitlab"),
            "{s}"
        );
        assert!(s.contains("y install"), "{s}");
        assert!(s.contains("n browse github"), "{s}");

        assert_eq!(
            popup.handle_key(key(KeyCode::Char('y'))),
            Action::DeclarationAccept
        );
        assert_eq!(
            popup.handle_key(key(KeyCode::Char('n'))),
            Action::DeclarationDecline
        );
        assert_eq!(
            popup.handle_key(key(KeyCode::Esc)),
            Action::DeclarationDecline
        );
        assert_eq!(popup.handle_key(key(KeyCode::Char('q'))), Action::Noop);
    }

    /// The pin fields surface: tag and sha both ride the source row.
    #[test]
    fn pins_surface_in_the_popup() {
        let mut popup = ConsentPopup::new(Declaration {
            tag: Some("v0.2.1".into()),
            sha: Some("deadbeef".into()),
            ..decl()
        });
        let s = screen(&mut popup);
        assert!(s.contains("pins"), "{s}");
        assert!(s.contains("tag v0.2.1"), "{s}");
        assert!(s.contains("sha256"), "{s}");
        assert!(s.contains("against your config's pin"), "{s}");
    }

    /// Worker state renders: installing progress, failure with the
    /// error and the honest escape hatch.
    #[test]
    fn installing_and_failed_states_render() {
        let mut popup = ConsentPopup::new(decl());
        popup.set_state(DeclarationState::Installing);
        assert!(screen(&mut popup).contains("installing — verified download running"));

        popup.set_state(DeclarationState::Failed("network: refused".into()));
        let s = screen(&mut popup);
        assert!(s.contains("failed: network: refused"), "{s}");
        assert!(s.contains("esc — browse github"), "{s}");
        // A failed install still declines cleanly.
        assert_eq!(
            popup.handle_key(key(KeyCode::Esc)),
            Action::DeclarationDecline
        );
    }
}
