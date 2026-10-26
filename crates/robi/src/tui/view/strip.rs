//! The status strip: one row between the transcript and the composer.
//!
//! Everything transient goes here, in one place, above the composer rather than
//! below it: the app-level error, the health mark, `sending…`/`stopping…`, and
//! the turn's own activity — `Thinking for 4s`, which is the webview's activity
//! line at the end of the transcript. One row, because only one of these is
//! usually true, and because a terminal that prints every status in a different
//! place makes the reader hunt for the one they want.
//!
//! Order is priority: an error that outlives a turn outranks the turn, and a
//! locked field outranks the phase it is about to move to.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::activity::PROGRESS_CELLS;
use crate::tui::theme;

/// The cell the progress widget draws. `▪`, one column wide.
pub const PROGRESS_CELL: char = '\u{25aa}';

/// Cells kept clear between the status and the progress widget.
const PROGRESS_GAP: usize = 2;

/// Three cells of dots, the webview's `ThinkingDots`, but at the tick pace and
/// in a fixed field so the text before it never shifts.
const DOTS_WIDTH: usize = 3;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let Some(mut line) = content(app) else {
        return;
    };

    // The marquee owns the right edge of the same row: the status says what is
    // happening, the widget says it is still moving. The status is clipped to
    // make room, never the other way round.
    let progress = progress_spans(app);
    let progress_width: usize = progress
        .iter()
        .map(|span| span.content.chars().count())
        .sum();
    let width = usize::from(area.width);
    if progress.is_empty() {
        line = clip_line(line, width);
    } else {
        let text_width = width.saturating_sub(progress_width + PROGRESS_GAP);
        line = clip_line(line, text_width);
        let used = line.width();
        let pad = width.saturating_sub(used + progress_width);
        line.spans.push(Span::raw(" ".repeat(pad)));
        line.spans.extend(progress);
    }
    frame.render_widget(Paragraph::new(line).style(theme::canvas()), area);
}

/// The progress widget: eight cells with one lit head, or nothing when idle.
///
/// A turn reports no percentage, so the head moves rather than filling: the
/// widget says "still working" and never claims to know how much is left.
fn progress_spans(app: &App) -> Vec<Span<'static>> {
    if !app.is_busy() {
        return Vec::new();
    }
    let head = app.activity().progress().unwrap_or(0);
    (0..PROGRESS_CELLS)
        .map(|cell| {
            let style = if cell == head {
                theme::progress_head()
            } else {
                theme::progress_track()
            };
            Span::styled(PROGRESS_CELL.to_string(), style)
        })
        .collect()
}

/// `line` cut to at most `width` cells, keeping the styles of what is left.
fn clip_line(line: Line<'static>, width: usize) -> Line<'static> {
    let mut kept: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in line.spans {
        if used >= width {
            break;
        }
        let mut text = String::new();
        for ch in span.content.chars() {
            let cell = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + cell > width {
                break;
            }
            used += cell;
            text.push(ch);
        }
        if !text.is_empty() {
            kept.push(Span::styled(text, span.style));
        }
    }
    Line::from(kept)
}

/// What the row says, or nothing when the app has nothing to report.
fn content(app: &App) -> Option<Line<'static>> {
    if let Some(error) = app.status().error() {
        return Some(Line::from(Span::styled(
            format!("! {error}"),
            theme::error(),
        )));
    }
    if let Some(reason) = app.status().health() {
        return Some(Line::from(Span::styled(
            format!("! health: {reason}"),
            theme::error(),
        )));
    }
    if app.is_stopping() {
        return Some(Line::from(Span::styled(
            "stopping\u{2026}",
            theme::composer_status(),
        )));
    }
    if app.composer().sending() {
        return Some(Line::from(Span::styled(
            "sending\u{2026}",
            theme::composer_status(),
        )));
    }
    let summary = app.activity().summary()?;
    // The dots sit in a fixed three-cell field so the text before them never
    // moves as the count changes, the same rule the waiting card follows.
    let dots = format!(
        "{:<width$}",
        ".".repeat(app.activity().dots()),
        width = DOTS_WIDTH
    );
    Some(Line::from(vec![
        Span::styled(format!("{summary} "), theme::activity()),
        Span::styled(dots, theme::activity()),
    ]))
}

#[cfg(test)]
mod tests {
    use crate::tui::app::Msg;
    use crate::tui::state::Phase;
    use crate::tui::testing;

    use super::*;

    #[test]
    fn an_idle_app_has_no_strip() {
        let (app, _, _) = testing::booted(80, 24);
        assert!(content(&app).is_none());
        assert_eq!(app.status_rows(), 0);
    }

    #[test]
    fn an_error_outranks_the_turn() {
        let (mut app, _, session) = testing::booted(80, 24);
        app.update(Msg::Agent(testing::turn_started(session)));
        app.update(Msg::Done(crate::tui::action::ActionOutcome::Failed {
            generation: None,
            message: "the store is locked".to_owned(),
        }));
        let line = content(&app).expect("a strip");
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "! the store is locked");
        assert_eq!(app.status_rows(), 1);
    }

    #[test]
    fn a_running_turn_reads_as_the_webviews_activity_line() {
        let (mut app, _, session) = testing::booted(80, 24);
        app.update(Msg::Agent(testing::turn_started(session)));
        let line = content(&app).expect("a strip");
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(
            text.trim_end(),
            "Thinking .",
            "one dot, in a three-cell field"
        );
        assert_eq!(app.activity().phase(), Phase::Thinking);
    }
}

#[cfg(test)]
mod order_tests {
    use crate::tui::app::Msg;
    use crate::tui::testing;

    use super::*;

    fn text(app: &App) -> Option<String> {
        content(app).map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
    }

    #[test]
    fn a_locked_field_outranks_the_activity_line_and_a_failure_outranks_both() {
        let (mut app, _, session) = testing::booted(80, 24);
        app.update(Msg::Agent(testing::turn_started(session)));
        assert!(text(&app).is_some_and(|line| line.starts_with("Thinking")));

        // A send in flight is the more immediate fact than the phase.
        app.update(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('h'),
            ratatui::crossterm::event::KeyModifiers::NONE,
        )));
        app.update(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Enter,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(text(&app).as_deref(), Some("sending\u{2026}"));

        app.update(Msg::Done(crate::tui::action::ActionOutcome::SubmitFailed {
            session,
            message: "the provider refused".to_owned(),
        }));
        assert_eq!(text(&app).as_deref(), Some("! the provider refused"));
    }
}
