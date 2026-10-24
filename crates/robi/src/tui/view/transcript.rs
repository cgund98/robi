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
use crate::tui::state::Phase;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let transcript = app.transcript();
    if transcript.is_empty() {
        draw_empty(frame, area, app);
        return;
    }
    let mut lines: Vec<Line<'_>> = transcript
        .visible()
        .iter()
        .map(|row| line_for(row, area.width))
        .collect();

    // A caret on the last row while the reply is arriving. It is appended to
    // the built line rather than drawn by the row builder, so the wrapped rows
    // and the scroll offset stay exactly what `state::transcript` computed.
    if transcript.phase() == Phase::Responding {
        if let Some(line) = lines.last_mut() {
            if line.width() < usize::from(area.width) {
                line.spans.push(Span::styled("\u{258d}", theme::caret()));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), area);
}

fn line_for(row: &Row, width: u16) -> Line<'_> {
    let (band, rail) = match row.kind {
        RowKind::User => (theme::user_bubble(), theme::user_rail()),
        RowKind::ToolHeading => (theme::tool_card_heading(), theme::tool_card_rail()),
        _ => (theme::tool_card(), theme::tool_card_rail()),
    };
    match row.kind {
        RowKind::User => band_line(row, width, band, rail),
        RowKind::ToolHeading => band_line(row, width, band, rail),
        RowKind::ToolCard => band_line(row, width, band, rail),
        RowKind::Assistant => spans_line(row, theme::prose()),
        RowKind::Divider => Line::from(Span::styled(
            format!("\u{2014} {} \u{2014}", row.text),
            theme::divider(),
        ))
        .alignment(Alignment::Center),
        RowKind::Spacer => Line::default(),
    }
}

/// A line of prose, painted from the row's own spans.
///
/// `base` is the tone the row wears where nothing finer is set: assistant prose
/// is `--ink-bright`, and a markdown span overrides only the properties it names
/// — so a heading underlines and brightens without losing the canvas.
fn spans_line(row: &Row, base: ratatui::style::Style) -> Line<'static> {
    if row.spans.is_empty() {
        return Line::from(Span::styled(row.text.clone(), base));
    }
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    for (range, style) in &row.spans {
        let start = range.start.min(row.text.len());
        let end = range.end.min(row.text.len());
        if start > cursor {
            spans.push(Span::styled(row.text[cursor..start].to_owned(), base));
        }
        if end > start {
            spans.push(Span::styled(
                row.text[start..end].to_owned(),
                base.patch(*style),
            ));
        }
        cursor = cursor.max(end);
    }
    if cursor < row.text.len() {
        spans.push(Span::styled(row.text[cursor..].to_owned(), base));
    }
    Line::from(spans)
}

/// One row of a band: the rail cell, then the content and its fill.
///
/// The row's spans are the byte ranges it wants painted in a finer tone — a
/// card's `$` marker — and are clamped so a stale range cannot panic.
fn band_line(
    row: &Row,
    width: u16,
    band: ratatui::style::Style,
    rail: ratatui::style::Style,
) -> Line<'static> {
    let rail_span = Span::styled(RAIL_CHAR.to_string(), rail);
    let body = row.text.strip_prefix(RAIL_CHAR).unwrap_or(&row.text);
    // Span ranges are measured against `row.text`, so shift them past the rail.
    let shift = row.text.len() - body.len();

    let mut spans = vec![rail_span];
    let mut cursor = 0usize;
    for (range, style) in &row.spans {
        let start = range.start.saturating_sub(shift).min(body.len());
        let end = range.end.saturating_sub(shift).min(body.len());
        if start > cursor {
            spans.push(Span::styled(body[cursor..start].to_owned(), band));
        }
        if end > start {
            spans.push(Span::styled(body[start..end].to_owned(), *style));
        }
        cursor = cursor.max(end);
    }
    if cursor < body.len() {
        spans.push(Span::styled(body[cursor..].to_owned(), band));
    }

    // The fill to the full band width: `width` cells, minus the rail, minus
    // what the text already covers.
    let fill = usize::from(width).saturating_sub(display_width(&row.text));
    if fill > 0 {
        spans.push(Span::styled(" ".repeat(fill), band));
    }
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
