//! Shared painting for the panels and views built from a list of rows.
//!
//! Two problems the first cut of the overlays had, both visible only on screen:
//!
//! - **The frame underneath showed through.** A `Block` paints a background
//!   style over its area, but `set_style` leaves the characters in place, so
//!   "No messages yet." was still legible behind the panel. [`fill`] clears the
//!   cells first.
//! - **The selected row had no visible background.** The text styles are built
//!   from the theme helpers, and each one carries `bg(BG_CANVAS)`; patching one
//!   over a selection style replaced the highlight with the canvas. [`row`]
//!   sets the background last, and pads the row to the panel's width, so the
//!   highlight is a band rather than the text's own width.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use ratatui::Frame;

use crate::tui::theme;

/// The background of a row that is not selected.
pub const ROW_BG: Color = theme::BG_CANVAS;
/// The background of the row the keys act on.
pub const SELECTED_BG: Color = theme::BG_SURFACE_ACTIVE;

/// Erase `area`, then paint it as canvas.
///
/// `Clear` is what removes the glyphs; the `Block` is what puts the canvas
/// colour back, because a cleared cell has no background of its own.
pub fn fill(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::canvas()), area);
}

/// The border every overlay wears: one hairline in `--ink-faint`, on the canvas.
///
/// A named constant rather than a style built at each call site, because two
/// overlays with two border tones is the exact drift this module exists to stop.
pub fn border() -> Block<'static> {
    Block::bordered()
        .border_style(Style::default().fg(theme::INK_FAINT))
        .style(theme::canvas())
}

/// A panel centred in `area`, filled, and titled — the shape every overlay uses.
///
/// Returns the panel's inner rect, or `None` when the frame cannot hold it. The
/// caller passes the size it needs; the panel is clamped to the frame and never
/// drawn smaller than a border and one row.
///
/// **This is the one place an overlay is painted.** A panel that cleared and a
/// panel that did not produced two different backgrounds for the same rows, and
/// the difference was only visible on screen; going through one function is what
/// makes "all overlays look alike" a property of the code rather than a habit.
pub fn panel(frame: &mut Frame, area: Rect, width: u16, height: u16, title: &str) -> Option<Rect> {
    let width = width.min(area.width);
    let height = height.min(area.height);
    if width < 8 || height < 3 {
        return None;
    }
    let panel = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    // Erase first: whatever was drawn here is still legible through a style.
    fill(frame, panel);
    let block = border().title(Span::styled(format!(" {title} "), theme::strong()));
    let inner = block.inner(panel);
    frame.render_widget(block, panel);
    (inner.height > 0).then_some(inner)
}

/// One row of a list: `spans`, on the row's background, padded to `width`.
///
/// The background is applied last, over whatever the span's own style carries,
/// so a selected row is one unbroken band and a theme helper cannot cancel it.
pub fn row(width: u16, selected: bool, spans: Vec<Span<'static>>) -> Line<'static> {
    let background = if selected { SELECTED_BG } else { ROW_BG };
    let mut used = 0usize;
    let mut painted: Vec<Span<'static>> = Vec::with_capacity(spans.len() + 1);
    for span in spans {
        used += Span::raw(span.content.clone()).width();
        let style = Style {
            bg: Some(background),
            ..span.style
        };
        painted.push(Span::styled(span.content, style));
    }
    let pad = usize::from(width).saturating_sub(used);
    if pad > 0 {
        painted.push(Span::styled(
            " ".repeat(pad),
            Style::default().bg(background),
        ));
    }
    Line::from(painted)
}
