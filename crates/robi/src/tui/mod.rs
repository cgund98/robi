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
//! `docs/src/design/shell/tui.md`; this module is the phase T0 skeleton.
//!
//! # Terminal lifecycle
//!
//! [`run`] owns the terminal: it refuses a non-TTY, takes the stopgap process
//! lock, initializes the alternate screen, and restores on every exit path. It
//! **does not** install the tracing subscriber: the binary calls
//! [`logs::init_tui`](crate::logs::init_tui) before it builds the app state, so
//! startup logging lands in the file and never on stderr.

pub mod action;
pub mod app;
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

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::time::Duration;

use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use crate::domain::events::EventSubscription;
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
}

/// Run the terminal front end until the user leaves.
///
/// The caller has already installed the TUI log layer; see the module docs.
pub async fn run(state: AppState, options: Options) -> io::Result<()> {
    if !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "robi-tui needs a terminal: stdout is not a TTY",
        ));
    }

    // Before the alternate screen: a refusal has to print on a normal terminal.
    let _lock = lifecycle::Lock::acquire(&options.state_dir, &options.database_url)?;

    let view = lifecycle::restore(&options.state_dir);
    let mut app = app::App::new(view, options.database_url.clone());

    let mut terminal = ratatui::init();
    // After raw mode, before the first frame: the wheel has to reach the pump.
    // A failure here restores first, so the shell is not left in raw mode.
    if let Err(error) = lifecycle::enable_mouse() {
        ratatui::restore();
        return Err(error);
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
    lifecycle::disable_mouse();
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
        terminal.draw(|frame| app.view(frame))?;
        if app.should_quit() {
            return Ok(());
        }
    }
}
