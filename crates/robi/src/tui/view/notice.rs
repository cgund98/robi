//! The footer: one line, the last notice or the key hint.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;

/// What the footer says when nothing has gone wrong.
pub const HINT: &str = "q quit \u{b7} PageUp/PageDown scroll";

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let (text, style) = match app.notice() {
        Some(notice) if notice.is_error() => (notice.text.clone(), theme::error()),
        Some(notice) => (notice.text.clone(), theme::info()),
        None => (HINT.to_owned(), theme::faint()),
    };
    // No leading space: the frame's gutter is the inset. A per-region pad here
    // would leave the footer one column in from the header.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, style))).style(theme::canvas()),
        area,
    );
}
