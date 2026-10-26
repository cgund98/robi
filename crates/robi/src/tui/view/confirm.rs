//! The confirmation, as a panel over the frame.
//!
//! A question that takes the keyboard should look like it takes the keyboard. It
//! used to be one row inserted between the composer and the footer, which read as
//! an annotation on the composer rather than as a decision the reader has to make —
//! and the row it took came out of the transcript, so asking about a compaction
//! moved the transcript under the reader's eyes.
//!
//! It is a panel for the reason the sessions list is: [`rows::panel`] gives it the
//! same border, fill and title as every other overlay, so `y` / `n` is answered on
//! a surface the reader has already met.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::view::rows;

/// The panel's width: a question and its detail on one line needs no more.
const PANEL_WIDTH: u16 = 56;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(confirm) = app.confirm() else {
        return;
    };
    // The question, the detail when there is one, and the keys. Two rows of text at
    // most, so the panel is a fixed height rather than a measurement.
    let height = 4 + u16::from(confirm.detail.is_some());
    let Some(inner) = rows::panel(frame, area, PANEL_WIDTH, height, "confirm") else {
        return;
    };
    let [question, _, keys] = Layout::vertical([
        Constraint::Length(if confirm.detail.is_some() { 2 } else { 1 }),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);

    let mut lines = vec![Line::from(Span::styled(
        confirm.question.clone(),
        theme::strong(),
    ))];
    if let Some(detail) = &confirm.detail {
        lines.push(Line::from(Span::styled(detail.clone(), theme::muted())));
    }
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), question);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "y confirm \u{b7} n cancel",
            theme::faint(),
        )))
        .style(theme::canvas()),
        keys,
    );
}
