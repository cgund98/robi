//! Terminal input, off the async runtime.
//!
//! `crossterm::event::read` blocks. On the runtime it would hold a worker for
//! as long as the user takes to press a key, so a dedicated `std::thread` reads
//! events and sends them into a bounded channel. A resize arrives on the same
//! channel, a wheel notch becomes [`Msg::Scroll`], and `SIGTERM` is forwarded as
//! [`Msg::Terminate`] so the terminal is restored on the way out.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyEventKind, MouseEventKind};
use tokio::sync::mpsc;

use crate::tui::app::Msg;

/// How many events may be queued before the reader thread waits.
///
/// The pump drains one message per pass, so a burst of key repeat is the only
/// thing this bound ever holds.
pub const INPUT_CAPACITY: usize = 64;

/// Rows one wheel notch scrolls.
///
/// Three is the usual terminal default: one notch is a "line" in the source
/// event but reads as too slow for prose, and a full page (the event's other
/// convention) makes the transcript jump past what the eye was following.
const WHEEL_ROWS: isize = 3;

/// The read end of the input channel.
pub struct Input {
    /// Messages from the reader thread and the signal watcher.
    pub rx: mpsc::Receiver<Msg>,
}

/// Start the reader thread and the `SIGTERM` watcher.
///
/// Call it after the terminal is in raw mode: `event::read` needs it, and a
/// thread that starts first can read a keystroke as a shell command.
pub fn spawn() -> io::Result<Input> {
    let (tx, rx) = mpsc::channel(INPUT_CAPACITY);

    let reader = tx.clone();
    std::thread::Builder::new()
        .name("robi-tui-input".to_owned())
        .spawn(move || read_loop(reader))?;

    watch_terminate(tx);
    Ok(Input { rx })
}

fn read_loop(tx: mpsc::Sender<Msg>) {
    loop {
        let message = match event::read() {
            Ok(event) => translate(event),
            // The terminal went away, or the process is exiting.
            Err(_) => return,
        };
        let Some(message) = message else {
            continue;
        };
        if tx.blocking_send(message).is_err() {
            // The pump dropped its receiver: the app is closing.
            return;
        }
    }
}

/// One terminal event as a message, or `None` when nothing listens.
///
/// Pulled out of the read loop so the mapping is a unit test: the loop blocks on
/// `event::read` and cannot be driven without a terminal.
pub fn translate(event: Event) -> Option<Msg> {
    match event {
        Event::Key(key) => {
            // Windows reports a release for every press. Only the press is a
            // keystroke.
            if key.kind == KeyEventKind::Release {
                return None;
            }
            Some(Msg::Key(key))
        }
        Event::Resize(width, height) => Some(Msg::Resize { width, height }),
        Event::FocusGained => Some(Msg::Focus(true)),
        Event::FocusLost => Some(Msg::Focus(false)),
        Event::Mouse(mouse) => scroll_rows(mouse.kind).map(Msg::Scroll),
        // Paste has no binding yet.
        _ => None,
    }
}

/// How far one mouse event scrolls the transcript, in rows.
///
/// `None` for anything that is not a wheel notch, so a click cannot be mistaken
/// for a small scroll. Up is negative — the transcript's own convention — so
/// the caller does not have to remember which way the offset runs.
pub fn scroll_rows(kind: MouseEventKind) -> Option<isize> {
    match kind {
        MouseEventKind::ScrollUp => Some(-WHEEL_ROWS),
        MouseEventKind::ScrollDown => Some(WHEEL_ROWS),
        MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => None,
        _ => None,
    }
}

/// `SIGTERM` closes the app the same way `q` does.
#[cfg(unix)]
fn watch_terminate(tx: mpsc::Sender<Msg>) {
    tokio::spawn(async move {
        let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        let Ok(mut terminate) = terminate else {
            tracing::warn!("could not listen for SIGTERM");
            return;
        };
        terminate.recv().await;
        let _ = tx.send(Msg::Terminate).await;
    });
}

/// No signal to watch. The reader thread stops on the next failed send.
#[cfg(not(unix))]
fn watch_terminate(_tx: mpsc::Sender<Msg>) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wheel is the newest input source, and its sign convention is the
    /// easiest thing to get backwards: up is negative, like every scroll in the
    /// transcript.
    #[test]
    fn focus_and_resize_become_messages_and_a_release_does_not() {
        use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

        assert_eq!(
            translate(Event::FocusLost),
            Some(Msg::Focus(false)),
            "a lost window is how a pause knows to ring"
        );
        assert_eq!(translate(Event::FocusGained), Some(Msg::Focus(true)));
        assert_eq!(
            translate(Event::Resize(80, 24)),
            Some(Msg::Resize {
                width: 80,
                height: 24
            })
        );
        let press = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(translate(Event::Key(press)), Some(Msg::Key(press)));
        assert_eq!(
            translate(Event::Key(KeyEvent {
                kind: KeyEventKind::Release,
                ..press
            })),
            None,
            "a release is not a second keystroke"
        );
    }

    #[test]
    fn a_wheel_notch_has_a_direction_and_nothing_else_does() {
        assert_eq!(scroll_rows(MouseEventKind::ScrollUp), Some(-WHEEL_ROWS));
        assert_eq!(scroll_rows(MouseEventKind::ScrollDown), Some(WHEEL_ROWS));

        // A click or a move is not a small scroll.
        for kind in [
            MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left),
            MouseEventKind::Up(ratatui::crossterm::event::MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ] {
            assert_eq!(scroll_rows(kind), None, "{kind:?}");
        }
    }
}
