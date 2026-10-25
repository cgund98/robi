//! The model and effort picker, as a panel over the frame.
//!
//! The same overlay shape the sessions and workspaces panels use —
//! [`rows::panel`], [`rows::row`] for every line — so a picker looks like the menus
//! it sits beside rather than like a second design.
//!
//! **Effort first.** It is three fixed rows, and a reader changing it should not
//! scroll past thirty models to reach them. The models follow, and the panel
//! scrolls to keep the selection visible.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::view::rows;

/// How wide the panel is, when the frame allows it.
const PANEL_WIDTH: u16 = 48;
/// The most inner rows the panel shows at once: two group headings, the effort
/// group, and a window onto the models.
const PANEL_ROWS: u16 = 14;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(picker) = app.picker() else {
        return;
    };
    let height = (PANEL_ROWS).min(picker.rows.len() as u16 + 2);
    let title = format!(
        "model and effort \u{b7} {} of {}",
        picker.selected + 1,
        picker.rows.len()
    );
    let Some(inner) = rows::panel(frame, area, PANEL_WIDTH, height + 2, &title) else {
        return;
    };
    let [list, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    // Keep the selection inside the window without a second scroll offset: the
    // list is short enough that an offset is more state than it is worth.
    let first = picker
        .selected
        .saturating_sub(list.height.saturating_sub(1) as usize);
    let lines: Vec<_> = picker
        .lines()
        .into_iter()
        .enumerate()
        .skip(first)
        .take(usize::from(list.height))
        .map(|(_, (label, selected, heading))| {
            if heading {
                // A heading is a full-width band like any other row, so the panel
                // has no ragged dark patch where the label ends.
                return rows::row(list.width, false, vec![Span::styled(label, theme::faint())]);
            }
            let style = if selected {
                theme::strong()
            } else {
                theme::prose()
            };
            rows::row(list.width, selected, vec![Span::styled(label, style)])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), list);
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::from(Span::styled(
            "Enter choose \u{b7} \u{2191}\u{2193} move \u{b7} Esc close",
            theme::faint(),
        )))
        .style(theme::canvas()),
        hint,
    );
}
