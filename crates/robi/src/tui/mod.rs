//! The terminal front end.
//!
//! A second front end over the same services the HTTP API drives: same
//! `AppState`, same sessions, same tools. In-process, no listener, no port. It
//! is behind the `tui` Cargo feature, so `robi-api` and `src-tauri` never
//! compile `ratatui`.
//!
//! The shape is Elm's: `update` is a pure function of state and a message,
//! `effects` runs the resulting [`action::Action`]s on the runtime, and the
//! pump re-renders after every message. The design is in
//! `docs/src/design/shell/tui.md`; this module is at phase T3: the transcript,
//! the composer, streaming, approvals on the selected card, the sessions and
//! workspace overlays, the status strip, and the per-turn duration.
//!
//! # Terminal lifecycle
//!
//! [`run`] owns the terminal: it refuses a non-TTY, initializes the alternate
//! screen, and restores on every exit path. There is no process-wide lock: two
//! terminals in one workspace are allowed, and what one may touch is decided per
//! session by `domain::chat_session::lease`. It
//! **does not** install the tracing subscriber: the binary calls
//! [`logs::init_tui`](crate::logs::init_tui) before it builds the app state, so
//! startup logging lands in the file and never on stderr.

pub mod action;
pub mod app;
pub mod args;
pub mod command;
pub mod effects;
pub mod input;
pub mod keys;
pub mod lifecycle;
pub mod persist;
pub mod state;
#[cfg(test)]
mod testing;
pub mod theme;
pub mod view;

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use crate::domain::events::EventSubscription;
use crate::tui::state::landing::Greeting;
use crate::web_api::state::AppState;

/// How often the pump ticks: spinners and elapsed time from T1, and the
/// catch-up reload. T0 only needs it so the loop has a bounded wait.
pub const TICK: Duration = Duration::from_millis(250);

/// What `run` needs beyond the services.
#[derive(Debug, Clone)]
pub struct Options {
    /// `~/.robi`: where `tui.json` and the stopgap `tui.lock` live.
    pub state_dir: PathBuf,
    /// Named in the lock file, so a second process can report which database
    /// the holder has open.
    pub database_url: String,
    /// The workspace to open: the CLI's path argument, or the working directory.
    ///
    /// The path decides the session list the reader starts with, and when it has
    /// no workspace the frame says so and offers to create it. It is canonical,
    /// so two spellings of one directory are one workspace.
    pub workspace_path: PathBuf,
}

/// Run the terminal front end until the user leaves.
///
/// The caller has already installed the TUI log layer; see the module docs.
pub async fn run(state: AppState, options: Options) -> io::Result<()> {
    if !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "robi needs a terminal: stdout is not a TTY",
        ));
    }

    let view = lifecycle::restore(&options.state_dir);
    // The greeting is read here, once: `update` and `view` stay pure functions of
    // the state they are given.
    let mut app = app::App::new(
        view,
        options.database_url.clone(),
        options.workspace_path.clone(),
        Greeting::now(),
        Arc::clone(&state.locks),
    );

    let mut terminal = ratatui::init();
    // After raw mode, before the first frame: the wheel has to reach the pump.
    // A failure here restores first, so the shell is not left in raw mode.
    if let Err(error) = lifecycle::enable_mouse() {
        ratatui::restore();
        return Err(error);
    }
    // Paste marking: without it a pasted block is typed keystroke by keystroke,
    // and its newlines send.
    if let Err(error) = lifecycle::enable_bracketed_paste() {
        tracing::warn!(%error, "could not turn bracketed paste on");
    }
    // Focus reporting is optional: a terminal that ignores it never sends the
    // events, and the most that costs is a bell that is not rung.
    if let Err(error) = lifecycle::enable_focus_change() {
        tracing::warn!(%error, "could not turn focus reporting on");
    }
    let size = terminal.size()?;
    app.resize(size.width, size.height);

    let input = input::spawn()?;
    let events = state.event_bus.subscribe();
    let (results, results_rx) = mpsc::channel(input::INPUT_CAPACITY);

    let outcome = pump(
        &mut terminal,
        &mut app,
        &state,
        &options,
        input.rx,
        events,
        results,
        results_rx,
    )
    .await;

    // Saved before the alternate screen goes, and awaited: a fire-and-forget
    // task would die with the runtime.
    effects::save_view(&options, &app.snapshot()).await;
    lifecycle::disable_focus_change();
    lifecycle::disable_bracketed_paste();
    lifecycle::disable_mouse();
    // Put the title back before the alternate screen goes, so the shell's own
    // title is not replaced by ours.
    lifecycle::set_title("");
    ratatui::restore();
    outcome
}

/// The one loop. Four sources, one `select!`, then draw.
#[allow(clippy::too_many_arguments)]
async fn pump(
    terminal: &mut DefaultTerminal,
    app: &mut app::App,
    state: &AppState,
    options: &Options,
    mut input: mpsc::Receiver<app::Msg>,
    mut events: EventSubscription,
    results: mpsc::Sender<app::Msg>,
    mut outcomes: mpsc::Receiver<app::Msg>,
) -> io::Result<()> {
    for action in app.boot() {
        effects::dispatch(state, options, action, &results);
    }
    let mut ticker = tokio::time::interval(TICK);
    let mut window_title = "";

    loop {
        tokio::select! {
            Some(message) = input.recv() => app.update(message),
            Some(envelope) = events.recv() => app.update(app::Msg::Agent(envelope)),
            _ = ticker.tick() => app.update(app::Msg::Tick),
            Some(outcome) = outcomes.recv() => app.update(outcome),
        }
        for action in app.take_actions() {
            effects::dispatch(state, options, action, &results);
        }
        // A bell and a title are writes to the terminal, not state, so they
        // happen here rather than in the pure `update`.
        if app.take_bell() {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(b"\x07");
            let _ = stdout.flush();
        }
        let title = app.title();
        if title != window_title {
            lifecycle::set_title(title);
            window_title = title;
        }
        terminal.draw(|frame| app.view(frame))?;
        if app.should_quit() {
            return Ok(());
        }
    }
}
