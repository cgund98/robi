//! The header: one row, `robi · workspace · session title`.
//!
//! The terminal's peer of the desktop window bar. It names the workspace and
//! the session, which is the whole navigation a T0 terminal has.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;

/// The separator between the three parts.
const SEPARATOR: &str = " \u{b7} ";

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(
        Paragraph::new(Line::from(spans(app))).style(theme::canvas()),
        area,
    );
}

fn spans(app: &App) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled("robi", theme::strong()),
        Span::styled(SEPARATOR, theme::faint()),
    ];
    if let Some(workspace) = app.active_workspace() {
        spans.push(Span::styled(workspace.name.clone(), theme::muted()));
        spans.push(Span::styled(SEPARATOR, theme::faint()));
    }
    spans.push(Span::styled(
        app.session_title().to_owned(),
        theme::strong(),
    ));
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_separator_is_the_design_docs_middle_dot() {
        assert_eq!(SEPARATOR, " \u{b7} ");
    }
}
