//! Terminal input, off the async runtime.
//!
//! `crossterm::event::read` blocks. On the runtime it would hold a worker for
//! as long as the user takes to press a key, so a dedicated `std::thread` reads
//! events and sends them into a bounded channel. A resize arrives on the same
//! channel, and `SIGTERM` is forwarded as [`Msg::Terminate`] so the terminal is
//! restored on the way out.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyEventKind};
use tokio::sync::mpsc;

use crate::tui::app::Msg;

/// How many events may be queued before the reader thread waits.
///
/// The pump drains one message per pass, so a burst of key repeat is the only
/// thing this bound ever holds.
pub const INPUT_CAPACITY: usize = 64;

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
            // Mouse, paste, and focus: no binding yet.
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
