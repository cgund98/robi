//! The terminal front end: `cargo run -p robi --features tui --bin robi`.
//!
//! The binary mirrors `robi-api`: environment to `AppConfig`, one `AppState`
//! from the composition root, then the front end. Three differences, all
//! deliberate:
//!
//! - The arguments are read first, so `--version` prints on a pipe. The TTY
//!   check comes after them, and before the log file and the half-opened
//!   database.
//! - The log layer is installed before the app state is built, and it is
//!   [`logs::init_tui`], which never writes to stderr — a tracing line there
//!   would overwrite the frame the terminal is drawing.
//! - The default database is the desktop app's **dev** database in a debug
//!   build, the same one `pnpm tauri dev` opens, and the app's own database in a
//!   release build. Otherwise a terminal started from a source checkout sees an
//!   empty session list while the desktop app shows a full one, and a downloaded
//!   binary writes a second database beside the one the app uses.
//!
//! `ROBI_DATABASE_URL` still wins, so any other database is one variable away.
//!
//! # The workspace
//!
//! `robi [PATH]` opens the workspace at `PATH`, or at the working directory
//! when no argument is given. The path decides which sessions the reader starts
//! with; a path with no workspace behind it is a prompt, not a silent fallback to
//! whichever workspace a previous run left open.

use std::io::IsTerminal;

use robi::bootstrap::{self, AppConfig};
use robi::tui::args::{self, Outcome};
use robi::tui::{self, Options};

/// The last resort: a file beside the working directory.
///
/// `robi-api` keeps this default, but the terminal only uses it when the
/// platform data directory cannot be resolved, because the desktop app it shares
/// sessions with does not.
const CWD_DATABASE_URL: &str = "sqlite://robi.db?mode=rwc";

/// Exit code for arguments the binary cannot act on.
///
/// 2, not 1: a bad invocation is a usage error, and a script can tell it apart
/// from a run that started and failed.
const BAD_USAGE: i32 = 2;

#[tokio::main]
async fn main() {
    // One optional argument: the workspace root. Anything else is a mistake worth
    // naming, because a terminal that ignores an argument opens the wrong chat.
    let workspace_path = match args::parse(std::env::args_os().skip(1)) {
        Outcome::Run(path) => path,
        Outcome::Help => {
            println!("{}", args::HELP);
            return;
        }
        Outcome::Version => {
            println!("{}", args::VERSION);
            return;
        }
        Outcome::Error(message) => {
            eprintln!("{message}");
            std::process::exit(BAD_USAGE);
        }
    };

    // After the flags, so `robi --version | cat` works. A pipe with no path
    // is still a mistake: the frame cannot be drawn into it.
    if !std::io::stdout().is_terminal() {
        eprintln!("robi needs a terminal: stdout is not a TTY");
        std::process::exit(1);
    }

    robi::logs::init_tui();

    let workspace_path = match workspace_path {
        Some(path) => path,
        None => match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(err) => {
                eprintln!("no workspace path and no working directory: {err}");
                std::process::exit(1);
            }
        },
    };
    // Canonical before the app sees it: the workspace row stores a canonical root,
    // and `..` or a symlink in the argument must still match it.
    let workspace_path = workspace_path
        .canonicalize()
        .unwrap_or_else(|_| workspace_path.clone());

    let database_url = match std::env::var("ROBI_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => match bootstrap::desktop_database_url() {
            Some(url) => {
                tracing::info!(%url, "using the desktop app's database");
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

    let outcome = tui::run(
        state,
        Options {
            state_dir,
            database_url,
            workspace_path,
        },
    )
    .await;
    // The terminal is restored by now, so this is the last thing the process
    // does: a container is a label in a daemon, not a file this process holds.
    bootstrap::shutdown_sandboxes().await;
    if let Err(err) = outcome {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
