//! The header: one row, `robi · workspace · session title`, with the activity
//! segment at the right edge.
//!
//! The terminal's peer of the desktop window bar. It names the workspace and
//! the session, and — from T1 — says what the open turn is doing and for how
//! long. The activity segment is right-aligned: the left side is identity and
//! does not move, the right side is state and changes every 250 ms.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;

/// The separator between the three parts.
const SEPARATOR: &str = " \u{b7} ";

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let mut line = Line::from(spans(app));
    if let Some(activity) = activity_span(app) {
        let used = line.width();
        let pad = usize::from(area.width)
            .saturating_sub(used)
            .saturating_sub(activity.content.len());
        if pad >= 2 {
            line.spans.push(Span::raw(" ".repeat(pad)));
            line.spans.push(activity);
        }
    }
    frame.render_widget(Paragraph::new(line).style(theme::canvas()), area);
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

/// `Thinking \u{b7} 4s \u{b7}\u{b7}` at the right edge, or nothing when idle.
///
/// The dots come from the tick count, so the segment moves without the view
/// keeping a timer of its own.
fn activity_span(app: &App) -> Option<Span<'static>> {
    let activity = app.activity();
    let summary = activity.summary()?;
    let dots = ".".repeat(activity.dots());
    Some(Span::styled(format!("{summary} {dots}"), theme::activity()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_separator_is_the_design_docs_middle_dot() {
        assert_eq!(SEPARATOR, " \u{b7} ");
    }
}
