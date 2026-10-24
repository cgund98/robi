//! The transcript region: the pre-wrapped rows, styled.
//!
//! No wrapping happens here. `state::transcript` produced one `Row` per visual
//! line, and `App` sliced the visible ones, so a scroll offset is row-exact.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::transcript::{display_width, wrap_text, Row, RowKind, RAIL_CHAR};
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let transcript = app.transcript();
    if transcript.is_empty() {
        draw_empty(frame, area, app);
        return;
    }
    let lines: Vec<Line<'_>> = transcript
        .visible()
        .iter()
        .map(|row| line_for(row, area.width))
        .collect();
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), area);
}

fn line_for(row: &Row, width: u16) -> Line<'_> {
    let (band, rail) = match row.kind {
        RowKind::User => (theme::user_bubble(), theme::user_rail()),
        RowKind::ToolHeading => (theme::tool_card_heading(), theme::tool_card_rail()),
        _ => (theme::tool_card(), theme::tool_card_rail()),
    };
    match row.kind {
        RowKind::User => band_line(row, width, band, rail, None),
        RowKind::ToolHeading => band_line(row, width, band, rail, None),
        RowKind::ToolCard => band_line(row, width, band, rail, row.muted.clone()),
        RowKind::Assistant => Line::from(Span::styled(row.text.as_str(), theme::prose())),
        RowKind::Divider => Line::from(Span::styled(
            format!("\u{2014} {} \u{2014}", row.text),
            theme::divider(),
        ))
        .alignment(Alignment::Center),
        RowKind::Spacer => Line::default(),
    }
}

/// One row of a band: the rail cell, then the content and its fill.
///
/// `muted` is a byte range of `row.text` drawn in the quieter tone — the `$` on
/// a card's subject line — and is clamped so a stale range cannot panic.
fn band_line(
    row: &Row,
    width: u16,
    band: ratatui::style::Style,
    rail: ratatui::style::Style,
    muted: Option<std::ops::Range<usize>>,
) -> Line<'static> {
    let rail_span = Span::styled(RAIL_CHAR.to_string(), rail);
    let body = row.text.strip_prefix(RAIL_CHAR).unwrap_or(&row.text);
    let fill = usize::from(width).saturating_sub(display_width(&row.text));
    let padded = format!("{body}{}", " ".repeat(fill));

    // A muted range is measured against `row.text`, so shift it past the rail.
    let shift = row.text.len() - body.len();
    let spans = match muted {
        Some(range) if range.end <= row.text.len() && range.start >= shift => {
            let start = range.start - shift;
            let end = range.end - shift;
            let mut spans = vec![rail_span];
            for (part, style) in [
                (&padded[..start], band),
                (&padded[start..end], theme::tool_card_marker()),
                (&padded[end..], band),
            ] {
                if !part.is_empty() {
                    spans.push(Span::styled(part.to_owned(), style));
                }
            }
            spans
        }
        _ => vec![rail_span, Span::styled(padded, band)],
    };
    Line::from(spans)
}

/// Why there is nothing to show, wrapped and centred.
///
/// A workspace list comes back empty for one of two reasons that look identical
/// from here: no workspace has ever been opened, or this process opened a
/// **different database** from the one the desktop app uses. The default
/// database is relative to the working directory, so starting the terminal in
/// another directory is enough to do it. Naming the database turns a puzzle
/// into a fact.
fn draw_empty(frame: &mut Frame, area: Rect, app: &App) {
    let lines = empty_lines(app, area.width);
    let height = (lines.len() as u16).min(area.height);
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(lines).style(theme::canvas()).centered(),
        middle,
    );
}

fn empty_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let texts: Vec<(&str, ratatui::style::Style)> = if app.active_workspace().is_none() {
        vec![
            ("No workspaces in this database.", theme::muted()),
            ("database:", theme::faint()),
            (app.database_url(), theme::faint()),
            (
                "Open one in the desktop app, or point ROBI_DATABASE_URL at its database.",
                theme::faint(),
            ),
        ]
    } else if app.open_session().is_none() {
        vec![("No sessions in this workspace yet.", theme::muted())]
    } else {
        vec![("No messages yet.", theme::muted())]
    };

    let mut lines = Vec::new();
    for (text, style) in texts {
        for wrapped in wrap_text(text, width) {
            lines.push(Line::from(Span::styled(wrapped, style)));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::persist::ViewState;
    use crate::tui::testing;

    #[test]
    fn the_empty_state_names_the_database_it_opened() {
        let mut app = App::new(ViewState::default(), "sqlite:///tmp/other.db");
        app.resize(80, 24);
        let frame = testing::screen(&app, 80, 24);
        assert!(frame.contains("No workspaces in this database"), "{frame}");
        assert!(
            frame.contains("sqlite:///tmp/other.db"),
            "the database has to be named, or an empty list is a puzzle: {frame}"
        );
        assert!(frame.contains("ROBI_DATABASE_URL"), "{frame}");
    }
}
