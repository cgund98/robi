//! The composer band's facts row: mode, model, provider, effort, and the meter.
//!
//! Left-aligned group, then the meter at the right edge. The row is on the band's
//! own surface, so it reads as part of the composer rather than as a second strip;
//! only the mode word carries a colour, because it is the one fact that changes and
//! the one `Ctrl-T` is about.
//!
//! Clipping is the state module's: [`Facts::fitted`] drops the least useful segment
//! first and never the mode.
//!
//! **Every span carries the band's background.** The theme's text helpers are built
//! for the canvas, so a `theme::faint()` separator on this row paints a
//! `--bg-canvas` patch in the middle of a `--surface` band, which reads as a dark
//! dot. [`on_band`] puts the band's own colour back, and every span goes through it.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::facts::Segment;
use crate::tui::theme;

/// The separator between two segments.
const SEPARATOR: &str = " \u{b7} ";

/// `style` on the composer band's own surface.
///
/// The theme's helpers carry `--bg-canvas`, which is deliberately darker than the
/// band. The background is patched last, the same rule `rows::row` follows and for
/// the same reason: a colour helper must not be able to punch a hole in a band.
fn on_band(style: ratatui::style::Style) -> ratatui::style::Style {
    ratatui::style::Style {
        bg: Some(theme::BG_SURFACE),
        ..style
    }
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let width = usize::from(area.width);
    let facts = app.facts();
    let (left, meter) = facts.split(width);
    let mut line = line_of(&left);

    // The meter owns the right edge. It is already dropped when the row is too
    // narrow, so this only has to pad.
    if let Some(meter) = meter {
        let text = meter.text();
        let used = line.width();
        let pad = width.saturating_sub(used + text.chars().count());
        if pad >= 2 {
            line.spans.push(Span::raw(" ".repeat(pad)));
            line.spans
                .push(Span::styled(text, on_band(theme::composer_status())));
        } else {
            // No room even for the gap: the left group keeps the row.
            line = line_of(&left);
        }
    }
    frame.render_widget(Paragraph::new(line).style(theme::composer_surface()), area);
}

fn line_of(segments: &[Segment]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(SEPARATOR, on_band(theme::faint())));
        }
        let style = match segment {
            Segment::Mode(_, mode) => theme::facts_mode(*mode),
            _ => theme::composer_status(),
        };
        spans.push(Span::styled(segment.text(), on_band(style)));
    }
    Line::from(spans)
}
