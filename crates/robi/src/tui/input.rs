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
            Ok(Event::Key(key)) => {
                // Windows reports a release for every press. Only the press is
                // a keystroke.
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                Msg::Key(key)
            }
            Ok(Event::Resize(width, height)) => Msg::Resize { width, height },
            Ok(Event::Mouse(mouse)) => match scroll_rows(mouse.kind) {
                Some(rows) => Msg::Scroll(rows),
                // A click, a drag, or a move: the view has no pointer target.
                None => continue,
            },
            // Paste and focus: no binding yet.
            Ok(_) => continue,
            // The terminal went away, or the process is exiting.
            Err(_) => return,
        };
        if tx.blocking_send(message).is_err() {
            // The pump dropped its receiver: the app is closing.
            return;
        }
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
