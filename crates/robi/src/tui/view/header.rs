//! The header: one row, `robi · workspace · session title` on the left and a
//! one-word health summary on the right.
//!
//! The terminal's peer of the desktop window bar. The left side is identity and
//! does not move; the right side is the state that changes — whether the
//! workspace's MCP servers are up. The turn's own state is not here: it lives at
//! the end of the transcript, where the webview puts it.

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
    if let Some((label, style)) = summary_span(app) {
        let used = line.width();
        let pad = usize::from(area.width)
            .saturating_sub(used)
            .saturating_sub(label.chars().count());
        if pad >= 2 {
            line.spans.push(Span::raw(" ".repeat(pad)));
            line.spans.push(Span::styled(label, style));
        }
    }
    frame.render_widget(Paragraph::new(line).style(theme::canvas()), area);
}

/// The rule under the header: one faint row of `\u{2500}` across the columns.
///
/// The frame draws no border anywhere else — a block is a band and a rail, not a
/// box — so this is one line and nothing more. It says what the row above is
/// (chrome) and where the page begins (the transcript), and it stops where the
/// header's own text stops, because `rule` is handed the content rect.
pub fn rule(frame: &mut Frame, area: Rect) {
    let cells = "\u{2500}".repeat(usize::from(area.width));
    let line = Line::from(cells).style(theme::divider());
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

/// `MCP Online` at the right edge, or nothing when the workspace has no servers.
///
/// One word for the whole workspace, as the top bar's tray is: a terminal has no
/// hover and no icons, so a per-server mark would say less in more cells. The
/// per-server detail is a status surface's job, not a one-row header's.
fn summary_span(app: &App) -> Option<(String, ratatui::style::Style)> {
    let (label, kind) = app.status().summary()?;
    Some((label, ratatui::style::Style::default().fg(kind.color())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_separator_is_the_design_docs_middle_dot() {
        assert_eq!(SEPARATOR, " \u{b7} ");
    }
}
