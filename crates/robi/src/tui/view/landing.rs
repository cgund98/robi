//! The landing page: the greeting, centred, with the composer beneath it.
//!
//! The terminal's peer of `ChatChrome.tsx`'s `.welcome` block: a greeting and a
//! composer in one centred group, with the empty frame around them. It is what a
//! workspace with no session open shows, and it is why `Ctrl-N` creates nothing —
//! the session row appears when the first message is sent.
//!
//! One group rather than a greeting floating over a bottom-pinned composer: the
//! two are one thing here (a question and the way to answer it), and separating
//! them would leave the page reading as an empty chat.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::view::composer;
use crate::tui::view::strip;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let band = app.composer().height();
    let strip_rows = app.status_rows();
    // The greeting, one blank row, the strip when there is something to say, and
    // the composer band.
    let group = 1u16
        .saturating_add(1)
        .saturating_add(strip_rows)
        .saturating_add(band);
    if area.height == 0 || area.width == 0 {
        return;
    }
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(group.min(area.height)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [greeting, _, strip_area, composer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(strip_rows),
        Constraint::Length(band),
    ])
    .areas(middle);

    // Green, like the desktop greeting: `--mode-ask`, the one green the tokens
    // have, so a new chat reads as the same colour in both front ends.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            app.greeting().label(),
            theme::landing_greeting(),
        )))
        .alignment(Alignment::Center)
        .style(theme::canvas()),
        greeting,
    );
    if strip_rows > 0 {
        strip::draw(frame, strip_area, app);
    }
    composer::draw(frame, composer_area, app);
}
