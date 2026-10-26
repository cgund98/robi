//! The command line: one optional workspace path, and the two flags every CLI
//! has.
//!
//! Kept out of `bin/robi.rs` so it is a pure function with tests: the
//! binary owns the TTY check, the logger, and the process exit, and this module
//! only decides what the arguments mean.
//!
//! `--version` matters more than it looks: the release workflow runs it on the
//! packaged tarball, so it proves both that the tarball runs and that the tag
//! and `CARGO_PKG_VERSION` agree.

use std::ffi::OsString;
use std::path::PathBuf;

/// What `--version` prints. Compared verbatim by the release smoke test.
pub const VERSION: &str = concat!("robi ", env!("CARGO_PKG_VERSION"));

pub const HELP: &str = "\
robi: the terminal front end

Usage:
  robi [OPTIONS] [PATH]

Arguments:
  PATH            The workspace to open. Defaults to the working directory.

Options:
  -h, --help      Print this help and exit.
  -V, --version   Print the version and exit.
  --              Treat every later argument as a path.

The database is the desktop app's dev database in a debug build, and the app's
database in a release build. ROBI_DATABASE_URL overrides both.";

/// What the arguments ask for.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Start the front end. `None` means no path was given, and the caller uses
    /// the working directory.
    Run(Option<PathBuf>),
    Help,
    Version,
    /// A mistake worth naming. The message is printed as written.
    Error(String),
}

/// Read the arguments after the program name.
///
/// `--` ends the flags, so a directory whose name starts with a dash has a
/// spelling. `-h` and `-V` win wherever they appear: a help request needs no
/// other argument to be valid.
pub fn parse<I>(args: I) -> Outcome
where
    I: IntoIterator<Item = OsString>,
{
    let mut path: Option<PathBuf> = None;
    let mut flags_done = false;

    for argument in args {
        if !flags_done {
            match argument.to_str() {
                Some("--") => {
                    flags_done = true;
                    continue;
                }
                Some("-h" | "--help") => return Outcome::Help,
                Some("-V" | "--version") => return Outcome::Version,
                Some(flag) if flag.starts_with('-') => {
                    return Outcome::Error(format!(
                        "robi does not take `{flag}`; `--help` lists what it does take"
                    ))
                }
                _ => {}
            }
        }

        if path.is_some() {
            return Outcome::Error(format!(
                "robi takes one workspace path, and got another: {}",
                PathBuf::from(argument).display()
            ));
        }
        path = Some(PathBuf::from(argument));
    }

    Outcome::Run(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Outcome {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn no_argument_runs_from_the_working_directory() {
        assert_eq!(parse_strs(&[]), Outcome::Run(None));
    }

    #[test]
    fn one_argument_is_the_workspace() {
        assert_eq!(
            parse_strs(&["/tmp/project"]),
            Outcome::Run(Some(PathBuf::from("/tmp/project")))
        );
    }

    #[test]
    fn both_spellings_of_help_and_version_work() {
        assert_eq!(parse_strs(&["-h"]), Outcome::Help);
        assert_eq!(parse_strs(&["--help"]), Outcome::Help);
        assert_eq!(parse_strs(&["-V"]), Outcome::Version);
        assert_eq!(parse_strs(&["--version"]), Outcome::Version);
    }

    #[test]
    fn a_flag_wins_over_a_path() {
        // A help request needs no other argument to be valid, and a terminal
        // started by mistake should be able to say so without opening anything.
        assert_eq!(parse_strs(&["/tmp/project", "--help"]), Outcome::Help);
    }

    #[test]
    fn a_second_path_is_named_not_ignored() {
        let outcome = parse_strs(&["/tmp/one", "/tmp/two"]);
        assert_eq!(
            outcome,
            Outcome::Error("robi takes one workspace path, and got another: /tmp/two".into())
        );
    }

    #[test]
    fn an_unknown_flag_is_an_error() {
        let outcome = parse_strs(&["--nope"]);
        let Outcome::Error(message) = outcome else {
            panic!("expected an error, got {outcome:?}");
        };
        assert!(message.contains("--nope"), "{message}");
    }

    #[test]
    fn a_double_dash_ends_the_flags() {
        // Otherwise a directory named `-h` could never be opened.
        assert_eq!(
            parse_strs(&["--", "-h"]),
            Outcome::Run(Some(PathBuf::from("-h")))
        );
        assert_eq!(
            parse_strs(&["--", "--version"]),
            Outcome::Run(Some(PathBuf::from("--version")))
        );
    }

    #[test]
    fn the_version_line_carries_the_crate_version() {
        assert_eq!(VERSION, concat!("robi ", env!("CARGO_PKG_VERSION")));
    }
}
