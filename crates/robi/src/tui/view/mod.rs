//! The frame: layout, the size guard, and one module per region.
//!
//! `draw` reads `App` and nothing else. It never mutates, never awaits, and
//! never decides anything a test cannot reproduce: the transcript rows were
//! wrapped in `state::transcript` and this module only styles them.

pub mod composer;
pub mod header;
pub mod notice;
pub mod transcript;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;

/// Below this, one centred sentence instead of a broken layout.
pub const MIN_WIDTH: u16 = 40;
/// Below this, one centred sentence instead of a broken layout.
pub const MIN_HEIGHT: u16 = 12;

/// Cells of canvas kept clear at the left and right of everything.
///
/// One gutter for the whole frame, not padding per region: the header, the
/// transcript, and the footer all share the same edges, so the screen reads as
/// one column with a margin rather than three boxes that happen to line up.
/// The transcript is wrapped to this width, so a card's band ends where the
/// header's text does.
pub const GUTTER: u16 = 2;

/// The columns the content occupies, after the gutter.
pub fn content_width(width: u16) -> u16 {
    width.saturating_sub(GUTTER * 2)
}

/// Rows of canvas between the transcript and the composer band.
///
/// The band is a raised surface; without a gap the last transcript row reads as
/// part of it. One row, the same "one blank row between blocks" the transcript
/// itself uses, so the spacing answers to one rule.
pub const COMPOSER_GAP: u16 = 1;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    // Paint the canvas first, so no cell keeps the terminal's own background.
    frame.render_widget(Block::default().style(theme::canvas()), area);

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        draw_too_small(frame, area);
        return;
    }

    let content = inner(area);
    // Five regions from T1 on: the header, the transcript, the gap, the composer
    // band, and the footer. The band's height comes from the field, which grows
    // with its text up to `MAX_COMPOSER_ROWS`, so `App::body_height` and this
    // layout agree by construction.
    let band = app.composer().height();
    let [top, body, _, composer, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(COMPOSER_GAP),
        Constraint::Length(band),
        Constraint::Length(1),
    ])
    .areas(content);

    header::draw(frame, top, app);
    transcript::draw(frame, body, app);
    // The gap is deliberately undrawn: the canvas is already painted.
    composer::draw(frame, composer, app);
    notice::draw(frame, bottom, app);
}

/// `area` inset by the gutter on both sides.
fn inner(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(GUTTER),
        width: content_width(area.width),
        ..area
    }
}

fn draw_too_small(frame: &mut Frame, area: Rect) {
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(
            "terminal too small \u{2014} 40x12 minimum"
                .to_string()
                .fg(theme::INK_MUTED),
        ))
        .style(theme::canvas())
        .centered(),
        middle,
    );
}
