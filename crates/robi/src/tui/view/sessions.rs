//! The sessions overlay: a panel over the transcript, a filter, and the rows.
//!
//! A panel, not a band: this surface owns the keyboard while it is up, so
//! covering the middle of the frame costs nothing and a bounded width keeps the
//! titles readable on a wide terminal. `Esc` closes it and the frame underneath
//! is exactly where it was.
//!
//! The first row is **New session**, the action a menu cannot otherwise offer.
//! It is painted like any other row and highlighted like any other row, so the
//! keyboard's target is never something that cannot be seen.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::sessions::{display_title, row_count, spinner, Mark, NEW_SESSION};
use crate::tui::theme;
use crate::tui::view::rows;

/// The panel's widest form. Wider than this and the eye has to cross the frame
/// to read one title.
const MAX_WIDTH: u16 = 64;
/// The border, the filter row, the border.
const CHROME_ROWS: u16 = 3;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let rows_now = app.session_rows();
    let visible_rows = usize::from(area.height.saturating_sub(CHROME_ROWS + 2).max(1));
    let width = area.width.min(MAX_WIDTH);
    let total = row_count(rows_now.len());
    let height = (total.min(visible_rows) as u16)
        .saturating_add(CHROME_ROWS)
        .min(area.height);
    if width < 8 || height < CHROME_ROWS {
        return;
    }
    let panel = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    // Erase first: the transcript's empty state is centred exactly here.
    rows::fill(frame, panel);
    let block = Block::bordered()
        .title(Span::styled(" sessions ", theme::strong()))
        .border_style(Style::default().fg(theme::INK_FAINT))
        .style(theme::canvas());
    let inner = block.inner(panel);
    frame.render_widget(block, panel);
    if inner.height == 0 {
        return;
    }

    let [filter_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    frame.render_widget(app.sessions_view().field(), filter_area);

    let selected = app.sessions_view().selected();
    // Keep the selection on screen without moving the list under it.
    let window = list_area.height as usize;
    let offset = if window == 0 {
        0
    } else {
        selected.saturating_sub(window.saturating_sub(1))
    };

    // Row zero is the action; the sessions follow it in the service's order.
    let mut lines: Vec<Line<'static>> = Vec::new();
    let action_row = rows::row(
        list_area.width,
        selected == 0,
        vec![Span::styled(format!("+ {NEW_SESSION}"), theme::prose())],
    );
    if offset == 0 && window > 0 {
        lines.push(action_row);
    }
    for (index, (session, mark)) in rows_now.iter().enumerate() {
        let row_index = index + 1;
        if row_index < offset || row_index >= offset + window {
            continue;
        }
        let (glyph, glyph_style) = mark_glyph(*mark, app.ticks());
        let mut spans = vec![
            Span::styled(glyph.to_string(), glyph_style),
            Span::styled(format!(" {}", display_title(session)), theme::prose()),
        ];
        if *mark == Mark::Awaiting {
            spans.push(Span::styled(
                "  waiting for approval",
                Style::default().fg(theme::ACCENT),
            ));
        }
        lines.push(rows::row(list_area.width, row_index == selected, spans));
    }

    // A filter that matches nothing still leaves the action row, so the way to
    // start a fresh chat is never hidden by a search.
    if rows_now.is_empty() {
        let message = if app.sessions().is_empty() {
            "no sessions in this workspace"
        } else {
            "no session matches the filter"
        };
        lines.push(Line::from(Span::styled(message, theme::faint())));
    }
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), list_area);
}

/// One cell per state, so a row never moves as a turn starts and stops.
fn mark_glyph(mark: Mark, ticks: u64) -> (char, Style) {
    match mark {
        Mark::Running => (spinner(ticks), Style::default().fg(theme::INK)),
        Mark::Awaiting => ('\u{25cf}', Style::default().fg(theme::ACCENT)),
        Mark::Idle => (' ', Style::default()),
    }
}
