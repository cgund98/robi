//! The composer band: the text field, on a raised surface with an accent rail.
//!
//! A band, not a box. It is the transcript's own affordance — one cell of rail
//! on the left edge, one cell of padding, no drawn border — so the frame has
//! one meaning for "this is a surface". The rail is `--accent` blue, the same
//! column a user message wears, because the composer *is* the user's turn
//! before it is sent. A tool card's rail is gray; this one is not machinery.
//!
//! Inside the band: a padding row, the text, and a padding row. Nothing else —
//! the status a locked field used to print here is now on the strip above the
//! band, with every other transient status, so the terminal has one place to
//! look rather than two. The mode and model row lands where that strip is from
//! T4.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::transcript::RAIL_CHAR;
use crate::tui::theme;
use crate::tui::view::facts;

/// Rows of padding above the text.
const PAD_TOP: u16 = 1;
/// Cells of padding between the rail and the text, and at the right edge.
///
/// Two the left: one is the band's own inset, the second is the field's. The
/// text never touches the rail, which is a rule, not a preference.
const PAD_X: u16 = 2;
/// One column of rail on the left edge.
const RAIL_W: u16 = 1;
/// The band's bottom: a blank row, the facts row, and a blank row under it.
///
/// The facts row is boxed in by a gap on both sides, so it reads as a caption on the
/// band rather than as the field's last line.
const FACTS_ROWS: u16 = 3;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width <= RAIL_W {
        return;
    }
    // The whole band, so the surface is unbroken behind the text.
    frame.render_widget(Paragraph::new("").style(theme::composer_surface()), area);

    // The rail runs the band's full height, top to bottom, exactly as a user
    // message's does. One column: the accent is the whole affordance.
    let rail = Rect {
        width: RAIL_W,
        ..area
    };
    let rail_lines: Vec<Line<'_>> = (0..area.height)
        .map(|_| {
            Line::from(Span::styled(
                RAIL_CHAR.to_string(),
                theme::composer_rail(app.agent_mode()),
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(rail_lines), rail);

    // Everything else sits one rail plus the field's horizontal padding in, and
    // the same padding off the right edge, so a long line wraps inside the band
    // rather than against it.
    let inner = Rect {
        x: area.x.saturating_add(RAIL_W),
        width: area.width.saturating_sub(RAIL_W),
        ..area
    };
    let content = Rect {
        x: inner.x.saturating_add(PAD_X),
        width: inner.width.saturating_sub(PAD_X * 2),
        ..inner
    };

    // The text: one padding row down, and stopping three rows above the band's
    // bottom edge. Those three are a blank row, the facts row — the mode, model,
    // provider, effort and the token meter — and a blank row under it. The gap
    // above separates what the composer *is* from what it will send, so the mode
    // line does not read as part of the message being typed; the gap below keeps it
    // off the band's edge, so it is a caption rather than a last line.
    let body = Rect {
        y: area.y.saturating_add(PAD_TOP),
        height: area
            .height
            .saturating_sub(PAD_TOP)
            .saturating_sub(FACTS_ROWS)
            .max(1),
        ..content
    };
    // `&TextArea` is a `Widget`: it draws the buffer and the placeholder.
    frame.render_widget(app.composer().textarea(), body);

    // The facts row: the band's second-to-last row, with a blank row under it.
    // Anchored to the band's bottom edge rather than to the text, so both gaps stay
    // put and the field can grow without moving it.
    let facts_area = Rect {
        y: area.y.saturating_add(area.height).saturating_sub(2),
        height: 1,
        ..content
    };
    if facts_area.y < area.y.saturating_add(area.height) {
        facts::draw(frame, facts_area, app);
    }

    // The real terminal cursor, only while the field owns the keyboard. A
    // second cursor drawn as text would sit still while the user typed.
    //
    // `cursor()` is a data coordinate. With `WrapMode::None` — the widget's
    // default, and the mode this field keeps — a data row is a screen row, so
    // the offset is exact.
    if app.focus().is_composer() && !app.composer().sending() {
        let cursor = app.composer().textarea().cursor();
        let x = body.x.saturating_add(cursor.1 as u16);
        let y = body.y.saturating_add(cursor.0 as u16);
        if x < body.x.saturating_add(body.width) && y < body.y.saturating_add(body.height) {
            frame.set_cursor_position((x, y));
        }
    }
}

/// The placeholder the empty field shows.
///
/// Re-exported for the snapshot test, so the rendered string and the assertion
/// cannot drift.
pub fn placeholder() -> &'static str {
    crate::tui::state::composer::PLACEHOLDER
}

/// The rail style, re-exported so a test can assert the accent without naming
/// the theme twice.
pub fn rail_style(mode: crate::domain::chat_session::model::AgentMode) -> Style {
    theme::composer_rail(mode)
}
