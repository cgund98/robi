//! The completion menu over the composer.
//!
//! It floats above the band, over the transcript's last rows, because it belongs at
//! the caret and it is one keystroke deep: reserving rows for it would move the
//! text being typed on every character.
//!
//! It is painted as a **panel**, not as a bare rectangle of coloured text:
//! [`rows::panel`] and [`rows::row`] are what the sessions and workspaces overlays
//! use, so the menu has their border and their row backgrounds. A menu that painted
//! its own background was a darker patch sitting over the transcript, which is the
//! kind of difference only the screen shows.

use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::view::rows;

/// The most rows the menu shows before the selection scrolls.
pub const MAX_ROWS: usize = 6;

/// The most columns the menu takes.
pub const MAX_WIDTH: u16 = 40;

/// The menu's rectangle, above `band_top`.
///
/// `None` when there is no room for it: a menu one row tall over a frame with no
/// transcript is worse than no menu, and the command can still be typed out.
pub fn area(band_top: u16, width: u16, rows: usize) -> Option<Rect> {
    if band_top < 4 || width == 0 || rows == 0 {
        return None;
    }
    // The rows, plus the border's two.
    let height = (rows.min(MAX_ROWS) as u16).saturating_add(2);
    if height + 1 > band_top {
        return None;
    }
    Some(Rect {
        x: 0,
        y: band_top.saturating_sub(height + 1),
        width: width.min(MAX_WIDTH),
        height,
    })
}

pub fn draw(frame: &mut Frame, band_top: u16, app: &App) {
    let Some(completion) = app.completion() else {
        return;
    };
    let width = crate::tui::view::content_width(frame.area().width);
    let Some(area) = area(band_top, width, completion.rows.len()) else {
        return;
    };
    let title = format!(
        "{} \u{b7} {} match{}",
        completion.sigil.char(),
        completion.rows.len(),
        if completion.rows.len() == 1 { "" } else { "es" }
    );
    // A panel anchored to the caret, not a centred one: `rows::panel` centres, so
    // this draws the same border and fill over a rect the caller placed.
    rows::fill(frame, area);
    let block = rows::border().title(Span::styled(format!(" {title} "), theme::strong()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }

    let lines: Vec<_> = completion
        .rows
        .iter()
        .enumerate()
        .skip(visible_from(completion.selected, completion.rows.len()))
        .take(usize::from(inner.height))
        .map(|(index, row)| {
            let selected = index == completion.selected;
            let mut spans = vec![Span::styled(
                format!(" {}", row.label()),
                if selected {
                    theme::strong()
                } else {
                    theme::prose()
                },
            )];
            if let Some(scope) = row.scope() {
                spans.push(Span::styled(format!(" {scope}"), theme::faint()));
            }
            let used: usize = spans
                .iter()
                .map(|span| Span::raw(span.content.clone()).width())
                .sum();
            let room = usize::from(inner.width).saturating_sub(used + 1);
            let detail: String = row.detail().chars().take(room).collect();
            if !detail.is_empty() {
                spans.push(Span::styled(format!(" {detail}"), theme::faint()));
            }
            rows::row(inner.width, selected, spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), inner);
}

/// The first row to draw, so the selection stays inside the menu.
fn visible_from(selected: usize, rows: usize) -> usize {
    if rows <= MAX_ROWS {
        return 0;
    }
    selected.saturating_sub(MAX_ROWS - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_sits_above_the_band_and_stays_short() {
        let area = area(20, 80, 3).expect("room for three rows");
        assert_eq!(area.height, 5, "three rows and the border");
        assert!(area.y + area.height < 20, "it ends above the band");
        assert_eq!(area.width, MAX_WIDTH, "and it does not span the frame");
    }

    #[test]
    fn a_long_list_is_capped_and_scrolls() {
        let area = area(20, 80, 30).expect("a capped menu");
        assert_eq!(area.height, MAX_ROWS as u16 + 2);
        assert_eq!(visible_from(0, 30), 0);
        assert_eq!(visible_from(9, 30), 4, "the selection is kept in view");
    }

    #[test]
    fn a_frame_with_no_room_gets_no_menu() {
        assert_eq!(area(2, 80, 5), None);
        assert_eq!(area(20, 0, 5), None);
        assert_eq!(area(20, 80, 0), None);
    }
}
