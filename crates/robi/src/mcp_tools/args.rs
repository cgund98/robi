//! The `mcp-tools` command line.
//!
//! Kept out of `bin/mcp-tools.rs` so it is a pure function with tests: the
//! binary owns the process exit and the printing, and this module only decides
//! what the arguments mean.

use std::ffi::OsString;
use std::path::PathBuf;

/// What `--version` prints.
pub const VERSION: &str = concat!("mcp-tools ", env!("CARGO_PKG_VERSION"));

pub const HELP: &str = "\
mcp-tools: list one configured MCP server's tools

Usage:
  mcp-tools [OPTIONS] <SERVER>

Arguments:
  SERVER               The server id in mcp.json.

Options:
  -h, --help           Print this help and exit.
  -V, --version        Print the version and exit.
  --home <DIR>         Robi's home directory. Defaults to $HOME/.robi.
  --workspace <DIR>    Read DIR/.robi/mcp.json too, and answer roots/list with
                       DIR. Defaults to the working directory, which reads only
                       the user file.
  --trust              Use the project file. Without it the project file is
                       skipped, the same as an untrusted workspace.
  --remote             Print the server's own tool names.
  --kept               Print only the tools the entry's `tools` filter keeps.
  --long               Append the first line of each description.
  --                   Treat every later argument as the server id.";

/// One accepted command line.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub server: String,
    pub home: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub trust: bool,
    pub remote: bool,
    pub kept: bool,
    pub long: bool,
}

/// What the arguments ask for.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    List(Options),
    Help,
    Version,
    /// A mistake worth naming. The message is printed as written.
    Error(String),
}

/// Read the arguments after the program name.
///
/// `--` ends the flags, so a server id starting with a dash has a spelling.
/// `-h` and `-V` win wherever they appear.
pub fn parse<I>(args: I) -> Outcome
where
    I: IntoIterator<Item = OsString>,
{
    let mut server: Option<String> = None;
    let mut home: Option<PathBuf> = None;
    let mut workspace: Option<PathBuf> = None;
    let mut trust = false;
    let mut remote = false;
    let mut kept = false;
    let mut long = false;
    let mut flags_done = false;

    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        if !flags_done {
            match argument.to_str() {
                Some("--") => {
                    flags_done = true;
                    continue;
                }
                Some("-h" | "--help") => return Outcome::Help,
                Some("-V" | "--version") => return Outcome::Version,
                Some("--trust") => {
                    trust = true;
                    continue;
                }
                Some("--remote") => {
                    remote = true;
                    continue;
                }
                Some("--kept") => {
                    kept = true;
                    continue;
                }
                Some("--long") => {
                    long = true;
                    continue;
                }
                Some("--home") => {
                    let Some(value) = args.next() else {
                        return Outcome::Error("--home needs a directory".into());
                    };
                    home = Some(PathBuf::from(value));
                    continue;
                }
                Some("--workspace") => {
                    let Some(value) = args.next() else {
                        return Outcome::Error("--workspace needs a directory".into());
                    };
                    workspace = Some(PathBuf::from(value));
                    continue;
                }
                Some(flag) if flag.starts_with('-') => {
                    return Outcome::Error(format!(
                        "mcp-tools does not take `{flag}`; `--help` lists what it does take"
                    ))
                }
                _ => {}
            }
        }

        if server.is_some() {
            return Outcome::Error(format!(
                "mcp-tools takes one server id, and got another: {}",
                argument.to_string_lossy()
            ));
        }
        server = Some(argument.to_string_lossy().into_owned());
    }

    let Some(server) = server else {
        return Outcome::Error("mcp-tools needs a server id; `--help` lists the options".into());
    };
    Outcome::List(Options {
        server,
        home,
        workspace,
        trust,
        remote,
        kept,
        long,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Outcome {
        parse(args.iter().map(OsString::from))
    }

    fn options(args: &[&str]) -> Options {
        match parse_strs(args) {
            Outcome::List(options) => options,
            other => panic!("expected a list, got {other:?}"),
        }
    }

    #[test]
    fn a_server_id_alone_is_a_list() {
        let parsed = options(&["linear"]);
        assert_eq!(parsed.server, "linear");
        assert!(parsed.home.is_none());
        assert!(parsed.workspace.is_none());
        assert!(!parsed.trust);
        assert!(!parsed.remote);
        assert!(!parsed.kept);
        assert!(!parsed.long);
    }

    #[test]
    fn every_flag_lands_where_it_belongs() {
        let parsed = options(&[
            "linear",
            "--home",
            "/tmp/home",
            "--workspace",
            "/tmp/work",
            "--trust",
            "--remote",
            "--kept",
            "--long",
        ]);
        assert_eq!(parsed.server, "linear");
        assert_eq!(parsed.home, Some(PathBuf::from("/tmp/home")));
        assert_eq!(parsed.workspace, Some(PathBuf::from("/tmp/work")));
        assert!(parsed.trust);
        assert!(parsed.remote);
        assert!(parsed.kept);
        assert!(parsed.long);
    }

    #[test]
    fn a_flag_may_follow_the_id() {
        assert!(options(&["linear", "--remote"]).remote);
        assert!(options(&["--remote", "linear"]).remote);
    }

    #[test]
    fn an_unknown_flag_is_an_error_naming_it() {
        match parse_strs(&["--nope", "linear"]) {
            Outcome::Error(message) => assert!(message.contains("--nope"), "{message}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn two_ids_are_an_error_naming_the_second() {
        match parse_strs(&["linear", "github"]) {
            Outcome::Error(message) => assert!(message.contains("github"), "{message}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn no_id_is_an_error() {
        assert!(matches!(parse_strs(&[]), Outcome::Error(_)));
        assert!(matches!(parse_strs(&["--remote"]), Outcome::Error(_)));
    }

    #[test]
    fn trust_alone_reads_the_project_file_of_the_working_directory() {
        // Without `--workspace` the root is the working directory, and its
        // `.robi/mcp.json` is still a project file worth trusting.
        assert!(options(&["linear", "--trust"]).trust);
        assert!(options(&["linear", "--trust", "--workspace", "/tmp"]).trust);
    }

    #[test]
    fn a_flag_needing_a_value_refuses_to_run_out_of_arguments() {
        assert!(matches!(
            parse_strs(&["linear", "--home"]),
            Outcome::Error(_)
        ));
        assert!(matches!(
            parse_strs(&["linear", "--workspace"]),
            Outcome::Error(_)
        ));
    }

    #[test]
    fn help_and_version_win_wherever_they_appear() {
        assert_eq!(parse_strs(&["linear", "--help"]), Outcome::Help);
        assert_eq!(parse_strs(&["--help"]), Outcome::Help);
        assert_eq!(parse_strs(&["-V"]), Outcome::Version);
    }

    #[test]
    fn a_double_dash_ends_the_flags() {
        assert_eq!(options(&["--", "--kept"]).server, "--kept");
    }

    #[test]
    fn the_version_line_carries_the_crate_version() {
        assert_eq!(VERSION, concat!("mcp-tools ", env!("CARGO_PKG_VERSION")));
    }
}
