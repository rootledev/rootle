//! Scope for GlobalSearch.

use super::{
    Block, Borders, Clear, Frame, GlobalSearch, Line, Modifier, Paragraph, Rect, Scope, Span,
    Style, Theme, centered_clamped,
};

impl GlobalSearch {
    pub(super) fn render_scope_popup(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let sem = &theme.semantic;
        let items = self.scope_items();
        let height = items.len() as u16 + 2; // rows + border
        let popup_area = centered_clamped(area, 40, 30, 24, 6);
        let popup = Rect {
            height: height.min(popup_area.height),
            ..popup_area
        };

        frame.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type())
            .border_style(Style::default().fg(sem.border_focused))
            .style(Style::default().bg(sem.mantle))
            .title(Span::styled(
                " scope ",
                Style::default().fg(sem.text).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                crate::keymap::hint_row(crate::keymap::search_scope_popup()),
                Style::default().fg(sem.hint),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let mut lines = Vec::new();
        for (idx, (scope, enabled)) in items.iter().enumerate() {
            let radio = if *scope == self.scope { "(•)" } else { "( )" };
            let label = match scope {
                Scope::Repo => match &self.repo {
                    Some(repo) => format!("current repo  repo:{repo}"),
                    None => "current repo  (no repo open)".to_string(),
                },
                Scope::Org => match &self.org {
                    Some(org) => format!("current org  org:{org}"),
                    None => "current org  (no org selected)".to_string(),
                },
                Scope::Global => "all repositories".to_string(),
            };
            let cursor = idx == self.scope_cursor;
            let fg = if !enabled {
                sem.subtext0
            } else if cursor {
                sem.selection_fg
            } else {
                sem.text
            };
            let mut style = Style::default().fg(fg);
            if cursor {
                style = style.bg(sem.selection_bg);
            }
            lines.push(Line::from(Span::styled(
                format!("{} {}", radio, label),
                style,
            )));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}
