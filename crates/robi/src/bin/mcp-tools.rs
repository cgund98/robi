//! `mcp-tools`: list one configured MCP server's tools.
//!
//! The command line is in [`robi::mcp_tools::args`] and the work is in
//! [`robi::mcp_tools::list`]. This binary owns the process: it reads the
//! arguments, builds the opener, prints the report, and returns the exit code.
//!
//! Exit codes: `0` printed the list, `1` could not read the config or connect,
//! `2` the arguments were wrong.

use robi::agent::mcp::RmcpOpener;
use robi::mcp_tools::args::{self, Outcome};
use robi::mcp_tools::{list, prepare};

/// A bad invocation. 2, so a script can tell it from a run that failed.
const BAD_USAGE: i32 = 2;

#[tokio::main]
async fn main() {
    let options = match args::parse(std::env::args_os().skip(1)) {
        Outcome::List(options) => options,
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

    let context = match prepare(options) {
        Ok(context) => context,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    // The same opener the app uses, so a stdio child gets the login PATH and the
    // config's environment, and an HTTP server gets its headers.
    let opener = RmcpOpener::new(
        context.home.clone(),
        std::sync::Arc::clone(&context.settings),
    );
    match list(&context, &opener).await {
        Ok(report) => {
            for line in report.lines {
                println!("{line}");
            }
            if let Some(note) = report.note {
                eprintln!("{note}");
            }
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
