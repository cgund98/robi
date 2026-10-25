//! The footer: one line, the last notice or the key hint, and the turn's
//! progress widget at the right edge.
//!
//! The widget is the terminal's peer of the desktop shell's running spinner: a
//! row of cells in `--ink-faint` with one `--ink` head moving along it. It sits
//! under the composer because that is where the eye already is while typing, and
//! at the right edge so it never competes with the notice on the left.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::activity::PROGRESS_CELLS;
use crate::tui::theme;

/// The hint for what is selected, or the composer's when nothing is.
fn hint(app: &App) -> &'static str {
    if let Some(call) = app.transcript().selection().and_then(|block| block.call) {
        let pending = app
            .approval()
            .is_some_and(|approval| approval.pending().contains(&call));
        if pending {
            return HINT_PENDING;
        }
        return HINT_CARD;
    }
    if app.transcript().selection().is_some() {
        // A paragraph: nothing to open, so nothing to name.
        return HINT_CARD;
    }
    HINT
}

/// The cell the progress widget draws. `▪`, one column wide.
pub const PROGRESS_CELL: char = '\u{25aa}';

/// Cells kept clear between the notice and the progress widget.
const PROGRESS_GAP: usize = 2;

/// What the footer says when nothing has gone wrong.
///
/// The composer's bindings, because that is where the keyboard is: `Enter`
/// sends, `Esc` stops a running turn, `Ctrl-J` is a newline, `Tab` reaches the
/// transcript, and `Ctrl-C` stops or, from idle, quits.
///
/// `Ctrl-J` first: some terminals deliver `Alt-Enter` as `Esc`, `Enter`, and
/// with `Esc` bound that sequence would interrupt and then send. `Alt-Enter`
/// still works where the terminal reports it as one key.
///
/// `q` is deliberately absent — it only quits with the transcript focused, and
/// a hint that named it would suggest typing it into the field is safe. The
/// footer clips on a narrow terminal.
pub const HINT: &str = "Enter send \u{b7} Esc stop \u{b7} Ctrl-J newline \u{b7} Tab focus \u{b7} \u{2191}\u{2193} select";

/// What the footer says when a card is selected.
///
/// Contextual because the actions are: `a` on a card that has already run does
/// nothing, and a hint that offered it anyway would be a lie the user has to test.
pub const HINT_CARD: &str = "Enter open \u{b7} \u{2191}\u{2193} select \u{b7} Esc clear";

/// What the footer says when the selected card is waiting on a decision.
///
/// This is the replacement for the modal's list of actions. Without it the pause
/// is answerable but invisible.
pub const HINT_PENDING: &str =
    "a approve \u{b7} r reject \u{b7} A approve all \u{b7} Enter open \u{b7} Ctrl-C decline";

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let (text, style) = match app.notice() {
        Some(notice) if notice.is_error() => (notice.text.clone(), theme::error()),
        Some(notice) => (notice.text.clone(), theme::info()),
        None => (hint(app).to_owned(), theme::faint()),
    };

    // The widget owns the right edge. The notice gets what is left, less the
    // gap, and is clipped to it: a hint that fills the row loses its tail, it
    // never pushes the progress widget off the screen.
    let progress = progress_spans(app);
    let progress_width: usize = progress
        .iter()
        .map(|span| span.content.chars().count())
        .sum();
    let gap = if progress.is_empty() { 0 } else { PROGRESS_GAP };
    let text_width = usize::from(area.width).saturating_sub(progress_width + gap);
    let text = clip(&text, text_width);

    let mut spans = vec![Span::styled(text.clone(), style)];
    if !progress.is_empty() {
        let used = text.chars().count();
        let pad = usize::from(area.width).saturating_sub(used + progress_width);
        spans.push(Span::raw(" ".repeat(pad)));
        spans.extend(progress);
    }

    // No leading space: the frame's gutter is the inset. A per-region pad here
    // would leave the footer one column in from the header.
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::canvas()),
        area,
    );
}

/// The progress widget: `▪▪▪▪▪▪▪▪` with one lit cell, or nothing when idle.
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

/// `text` cut to at most `width` cells.
fn clip(text: &str, width: usize) -> String {
    let mut clipped = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cell = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cell > width {
            break;
        }
        used += cell;
        clipped.push(ch);
    }
    clipped
}
