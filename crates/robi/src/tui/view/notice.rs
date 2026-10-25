//! The footer: one line, the last action's answer or the key hint.
//!
//! State lives on the strip above the composer, not here: the footer under the
//! band is what a key will do next, and the strip is what the app is doing now.
//! The turn's progress widget moved to the strip with every other status.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;

/// The hint for what is selected, or the composer's when nothing is.
fn hint(app: &App) -> &'static str {
    // A prompt or a confirmation owns the keyboard, so it names its own keys
    // before anything else: a hint that still offered chat's would be wrong.
    if app.confirm().is_some() {
        return HINT_CONFIRM;
    }
    if let Some(prompt) = app.prompt() {
        return prompt.kind.hint();
    }
    if app.sessions_open() {
        return HINT_SESSIONS;
    }
    if app.workspaces_open() {
        use crate::tui::state::workspaces::Screen;
        return match app.workspaces_view().screen {
            Screen::Recent => HINT_WORKSPACES,
            Screen::New => HINT_NEW,
        };
    }
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

/// While the sessions overlay is up.
pub const HINT_SESSIONS: &str =
    "Enter open \u{b7} Ctrl-N new \u{b7} Ctrl-R rename \u{b7} Ctrl-D delete \u{b7} Esc close";

/// The workspace overlay's recent list.
pub const HINT_WORKSPACES: &str =
    "Enter open \u{b7} \u{2191}\u{2193} select \u{b7} Ctrl-N new \u{b7} Esc close";

/// The workspace overlay's create screen.
pub const HINT_NEW: &str =
    "Enter create \u{b7} Tab complete \u{b7} \u{2191}\u{2193} pick \u{b7} Esc back";

/// The confirmation above an overlay.
pub const HINT_CONFIRM: &str = "y confirm \u{b7} n cancel";

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let (text, style) = match app.notice() {
        Some(notice) if notice.is_error() => (notice.text.clone(), theme::error()),
        Some(notice) => (notice.text.clone(), theme::info()),
        None => (hint(app).to_owned(), theme::faint()),
    };
    let text = clip(&text, usize::from(area.width));
    // No leading space: the frame's gutter is the inset. A per-region pad here
    // would leave the footer one column in from the header.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, style))).style(theme::canvas()),
        area,
    );
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
