//! The help page: title, sections, hint — the shape `manage` and `detail` use.
//!
//! It owns the frame the way those two do, because a reader who opened it went
//! somewhere on purpose. Rows are `keys` padded to the widest key string in the
//! page, so the descriptions line up down the whole list rather than per section.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::view::GUTTER;

/// Rows a section title takes: the title and a blank line under it.
const TITLE_ROWS: usize = 2;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let inner = Rect {
        x: area.x.saturating_add(GUTTER),
        width: area.width.saturating_sub(GUTTER * 2),
        ..area
    };
    let [top, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("\u{2039} Esc", theme::muted()),
            Span::raw("  "),
            Span::styled("help", theme::strong()),
        ]))
        .style(theme::canvas()),
        top,
    );

    let rows = crate::tui::state::help::page();
    let key_width = rows
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|(keys, _)| keys.chars().count())
        .max()
        .unwrap_or(0)
        .min(usize::from(inner.width).saturating_sub(8));

    // Flatten to display rows: a section title, then its entries.
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (title, entries) in &rows {
        lines.push(Line::from(Span::styled(title.to_owned(), theme::strong())));
        lines.push(Line::default());
        for (keys, what) in entries {
            let padded = format!("{keys:<key_width$}");
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(padded, theme::muted()),
                Span::raw("  "),
                Span::styled((*what).to_owned(), theme::prose()),
            ]));
        }
        lines.push(Line::default());
    }

    let visible: Vec<Line<'static>> = lines
        .into_iter()
        .skip(app.help().offset)
        .take(usize::from(body.height).max(1))
        .collect();
    frame.render_widget(Paragraph::new(visible).style(theme::canvas()), body);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Esc back \u{b7} \u{2191}\u{2193} scroll \u{b7} PageUp PageDown \u{b7} Home End",
            theme::faint(),
        )))
        .style(theme::canvas()),
        bottom,
    );
}

/// How many display rows the whole page takes, for the scroll's bounds.
pub fn row_count() -> usize {
    crate::tui::state::help::page()
        .iter()
        .map(|(_, entries)| entries.len() + TITLE_ROWS)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_longer_than_a_frame() {
        // If a section were ever removed this would catch a page that no longer
        // needs to scroll, which would make the scroll tests meaningless.
        assert!(row_count() > 20, "{} rows", row_count());
    }
}
