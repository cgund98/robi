//! The one-row prompt between the composer and the footer: the reject reason and
//! the rename field.
//!
//! One row because it is an aside, not a screen: the transcript stays where it
//! was, the composer stays visible, and the field is unmistakably the keyboard's
//! from the footer hint and from the caret.
//!
//! A **confirmation is not here**: it takes the keyboard for a question, so it is a
//! panel over the frame (`view/confirm.rs`), the shape every other overlay uses.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let Some(prompt) = app.prompt() else {
        return;
    };
    let label = format!("{}: ", prompt.label);
    let label_width = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
    if label_width >= area.width {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(label, theme::muted()))).style(theme::canvas()),
            area,
        );
        return;
    }
    let [label_area, field_area] =
        Layout::horizontal([Constraint::Length(label_width), Constraint::Min(1)]).areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(label, theme::muted()))).style(theme::canvas()),
        label_area,
    );
    // `&TextArea` is a `Widget`: it draws the buffer, and the caret is the real
    // terminal cursor so it blinks where the next character lands.
    frame.render_widget(&prompt.field, field_area);
    let cursor = prompt.field.cursor();
    let x = field_area.x.saturating_add(cursor.1 as u16);
    let y = field_area.y.saturating_add(cursor.0 as u16);
    if x < field_area.x.saturating_add(field_area.width) && y < field_area.y.saturating_add(1) {
        frame.set_cursor_position((x, y));
    }
}
