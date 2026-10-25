//! The terminal front end: `cargo run -p robi --features tui --bin robi-tui`.
//!
//! The binary mirrors `robi-api`: environment to `AppConfig`, one `AppState`
//! from the composition root, then the front end. Three differences, all
//! deliberate:
//!
//! - The TTY check comes first, so a pipe gets one line on stderr instead of a
//!   log file and a half-opened database.
//! - The log layer is installed before the app state is built, and it is
//!   [`logs::init_tui`], which never writes to stderr — a tracing line there
//!   would overwrite the frame the terminal is drawing.
//! - The default database is the desktop app's **dev** database, the same one
//!   `pnpm tauri dev` opens, rather than a file relative to the working
//!   directory. Otherwise a terminal started from a source checkout sees an
//!   empty session list while the desktop app shows a full one.
//!
//! `ROBI_DATABASE_URL` still wins, so any other database is one variable away.
//!
//! # The workspace
//!
//! `robi-tui [PATH]` opens the workspace at `PATH`, or at the working directory
//! when no argument is given. The path decides which sessions the reader starts
//! with; a path with no workspace behind it is a prompt, not a silent fallback to
//! whichever workspace a previous run left open.

use std::path::PathBuf;

use robi::bootstrap::{self, AppConfig};
use robi::tui::{self, Options};

/// The last resort: a file beside the working directory.
///
/// `robi-api` keeps this default, but the terminal only uses it when the
/// platform data directory cannot be resolved, because the desktop app it shares
/// sessions with does not.
const CWD_DATABASE_URL: &str = "sqlite://robi.db?mode=rwc";

#[tokio::main]
async fn main() {
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        eprintln!("robi-tui needs a terminal: stdout is not a TTY");
        std::process::exit(1);
    }

    robi::logs::init_tui();

    // One optional argument: the workspace root. Anything else is a mistake worth
    // naming, because a terminal that ignores an argument opens the wrong chat.
    let mut args = std::env::args_os().skip(1);
    let workspace_path = match args.next() {
        Some(argument) => PathBuf::from(argument),
        None => match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(err) => {
                eprintln!("no workspace path and no working directory: {err}");
                std::process::exit(1);
            }
        },
    };
    if let Some(extra) = args.next() {
        eprintln!(
            "robi-tui takes one workspace path, and got another: {}",
            PathBuf::from(extra).display()
        );
        std::process::exit(1);
    }
    // Canonical before the app sees it: the workspace row stores a canonical root,
    // and `..` or a symlink in the argument must still match it.
    let workspace_path = workspace_path
        .canonicalize()
        .unwrap_or_else(|_| workspace_path.clone());

    let database_url = match std::env::var("ROBI_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => match bootstrap::desktop_dev_database_url() {
            Some(url) => {
                tracing::info!(%url, "using the desktop app's dev database");
                url
            }
            None => {
                tracing::warn!(
                    url = CWD_DATABASE_URL,
                    "could not resolve the platform data directory"
                );
                CWD_DATABASE_URL.to_owned()
            }
        },
    };
    let state_dir = match robi::adapters::settings::home_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let state = match bootstrap::build_app_state(AppConfig {
        database_url: database_url.clone(),
        settings_dir: state_dir.clone(),
    })
    .await
    {
        Ok(state) => state,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = tui::run(
        state,
        Options {
            state_dir,
            database_url,
            workspace_path,
        },
    )
    .await
    {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
