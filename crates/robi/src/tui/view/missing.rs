//! The startup prompt: the path has no workspace behind it.
//!
//! `robi-tui ~/code/pocket` names a directory. When no workspace row has that
//! root, the frame says so and offers the two answers that exist — create it
//! here, or go and pick another — instead of silently opening whichever
//! workspace an earlier run left open. Silence there is the bug: the session list
//! would be somebody else's.
//!
//! **It stays the page when the switcher opens over it.** Choosing another
//! workspace opens the switcher as a panel on top of this, so closing it without
//! choosing returns to the question rather than to a chat frame with nothing
//! open. While the panel is up the page is drawn dimmed, and without its key
//! line: the panel owns the keyboard, so a `y`/`n` prompt the reader cannot use
//! would be a lie.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::transcript::wrap_text;
use crate::tui::theme;
use crate::tui::view::GUTTER;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let width = usize::from(area.width.saturating_sub(GUTTER * 2)).max(1);
    let path = app.target_workspace();
    // A panel over this page is the switcher, and it has the keyboard.
    let covered = app.workspaces_open();
    let (heading, subject, body) = if covered {
        (theme::muted(), theme::faint(), theme::faint())
    } else {
        (theme::strong(), theme::prose(), theme::muted())
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(
        "No workspace at this path.",
        heading,
    )));
    for wrapped in wrap_text(&path, width) {
        lines.push(Line::from(Span::styled(wrapped, subject)));
    }
    lines.push(Line::default());
    for wrapped in wrap_text(
        "Create a workspace here, or choose one of the workspaces this database already has.",
        width,
    ) {
        lines.push(Line::from(Span::styled(wrapped, body)));
    }
    if !covered {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("y", theme::strong()),
            Span::styled(" create it here", theme::muted()),
            Span::styled("   \u{b7}   ", theme::faint()),
            Span::styled("n", theme::strong()),
            Span::styled(" choose another", theme::muted()),
        ]));
    }

    let height = (lines.len() as u16).min(area.height);
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(Rect {
        x: area.x.saturating_add(GUTTER),
        width: area.width.saturating_sub(GUTTER * 2),
        ..area
    });
    frame.render_widget(
        Paragraph::new(lines).style(theme::canvas()).centered(),
        middle,
    );
}
