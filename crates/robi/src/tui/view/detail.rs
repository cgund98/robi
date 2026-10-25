//! One tool call, read in full: a header, the body, and its own scroll.
//!
//! The transcript's card is a summary; this is the rest of it. It replaces the
//! in-card expansion T2 shipped, which grew the band in place and could push the
//! turn it belonged to off the screen.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::tool_card::CardLineKind;
use crate::tui::theme;
use crate::tui::view::GUTTER;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(detail) = app.detail() else {
        return;
    };
    let content = Rect {
        x: area.x.saturating_add(GUTTER),
        width: area.width.saturating_sub(GUTTER * 2),
        ..area
    };
    // A title row and a footer row, exactly like the chat frame, so the reader
    // knows where they are and how to get back.
    let [top, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(content);

    let position = if detail.is_empty() {
        "empty".to_owned()
    } else {
        format!(
            "{}-{} of {}",
            detail.offset + 1,
            (detail.offset + app.detail_height()).min(detail.len()),
            detail.len()
        )
    };
    let title = Line::from(vec![
        Span::styled("\u{2039} back \u{b7} Esc", theme::muted()),
        Span::raw("  "),
        Span::styled(position, theme::faint()),
    ]);
    frame.render_widget(Paragraph::new(title).style(theme::canvas()), top);

    let lines: Vec<Line<'static>> = detail
        .visible(app.detail_height())
        .iter()
        .map(|line| {
            let style = match line.kind {
                CardLineKind::DiffAdd => Style::default().fg(theme::DIFF_ADD),
                CardLineKind::DiffDel => Style::default().fg(theme::DIFF_DEL),
                CardLineKind::Error => Style::default().fg(theme::DANGER),
                CardLineKind::More | CardLineKind::Hint => theme::faint(),
                CardLineKind::Body => theme::prose(),
            };
            Line::from(Span::styled(line.text.clone(), style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), body);

    // The body has no border, so the footer is the only frame edge; it is also
    // where the scroll keys are named, since this view has its own.
    let hint = Line::from(Span::styled(
        "\u{2191}\u{2193} j k scroll \u{b7} PgUp PgDn page \u{b7} Ctrl-U Ctrl-D half \
         \u{b7} Home End ends",
        theme::faint(),
    ));
    frame.render_widget(Paragraph::new(hint).style(theme::canvas()), bottom);
}

#[cfg(test)]
mod tests {
    use robi_core::message::Message;

    use crate::tui::app::Msg;
    use crate::tui::testing;

    use super::*;

    fn opened(output: &str) -> App {
        let (mut app, _, session) = testing::booted(80, 24);
        let mut call = testing::pending_call("shell", serde_json::json!({ "command": "seq" }));
        call.execution_status = robi_core::message::ExecutionStatus::Succeeded;
        call.approval_status = robi_core::message::ApprovalStatus::Approved;
        call.result = Some(serde_json::json!({ "stdout": output }));
        testing::land(
            &mut app,
            session,
            vec![Message::assistant_with_tool_calls("", vec![call])],
        );
        // Focus the transcript, select the card, open it.
        app.update(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Tab,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )));
        app.update(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Down,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )));
        app.update(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Enter,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )));
        app
    }

    #[test]
    fn the_view_replaces_the_frame_and_shows_the_whole_body() {
        let body: String = (0..80).map(|i| format!("line {i}\n")).collect();
        let app = opened(&body);

        assert_eq!(app.mode(), crate::tui::state::Mode::Detail);

        let screen = testing::screen(&app, 80, 24);
        assert!(screen.contains("# shell"), "{screen}");
        // Line 0 is on screen and the composer is not: the view owns the frame.
        assert!(screen.contains("line 0"), "{screen}");
        assert!(
            !screen.contains("Message robi"),
            "the composer is not drawn under it: {screen}"
        );
        // The position the reader is at, and the way back. The viewport is the
        // frame minus its two chrome rows, and it has to agree with the body.
        assert!(screen.contains("1-22 of 83"), "{screen}");
        assert!(screen.contains("Esc"), "{screen}");
    }

    #[test]
    fn scrolling_moves_the_position_and_the_arrows_reach_the_body() {
        let body: String = (0..80).map(|i| format!("line {i}\n")).collect();
        let mut app = opened(&body);
        let first = testing::screen(&app, 80, 24);
        assert!(first.contains("line 0"), "{first}");

        app.update(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::PageDown,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )));
        let second = testing::screen(&app, 80, 24);
        assert!(!second.contains("line 0"), "the body moved: {second}");
        assert!(second.contains("line 30"), "{second}");
    }
}
