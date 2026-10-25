//! Hand one workspace file to a program the user configured.
//!
//! The peer of `POST /workspaces/{id}/open`, shared with the terminal's `/open`.
//! The template is `file_open_command`, its `{file_path}` and `{line}`
//! placeholders are rendered by [`crate::domain::settings::open_command`], and
//! nothing here falls back to `$EDITOR` or to a system opener: an unset command
//! is the caller's refusal, which is a decision this module does not have.
//!
//! **Nothing is spawned by [`argv_for`].** That is the half a test can run, so
//! the resolution rules are asserted without starting a program.

use std::path::{Path, PathBuf};

use super::paths::absolute_inside;

/// Why a path, or the command that would open it, was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// The path is not a file inside the workspace.
    Path(super::paths::PathError),
    /// The stored template cannot be rendered.
    Template(String),
    /// The program could not be started, or exited non-zero inside the window.
    Spawn(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(error) => write!(f, "{error}"),
            Self::Template(message) | Self::Spawn(message) => f.write_str(message),
        }
    }
}

impl From<super::paths::PathError> for OpenError {
    fn from(error: super::paths::PathError) -> Self {
        Self::Path(error)
    }
}

/// The argv for one open of `relative` inside `root`.
///
/// The whole resolution and rendering, with no process started. `line` is the
/// 1-based line to land on, or `None` when the click had none.
pub fn argv_for(
    root: &Path,
    relative: &str,
    line: Option<u32>,
    template: &str,
) -> Result<Vec<String>, OpenError> {
    let absolute: PathBuf = absolute_inside(root, relative)?;
    let argv = crate::domain::settings::open_command::render_open_command(
        template,
        &absolute.display().to_string(),
        line,
    )
    .map_err(OpenError::Template)?;
    Ok(argv)
}

/// Resolve `relative`, render the template, and start the program.
///
/// Blocking: it waits for an early exit. Callers run it off the async runtime,
/// through `agent::blocking::call`.
pub fn open(
    root: &Path,
    relative: &str,
    line: Option<u32>,
    template: &str,
) -> Result<(), OpenError> {
    let argv = argv_for(root, relative, line, template)?;
    let program = argv[0].clone();
    let args = &argv[1..];
    spawn_opener(&program, args).map_err(OpenError::Spawn)
}

/// Start `program` and wait up to 10 seconds.
///
/// A process still running at the deadline is left running. A thread reaps it
/// later so it does not stay a zombie, and drains stderr so a full pipe cannot
/// stall the editor. A non-zero exit inside the window is an error.
fn spawn_opener(program: &str, args: &[String]) -> Result<(), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| io_message(&err, program))?;
    let deadline = Duration::from_secs(crate::domain::settings::open_command::OPEN_TIMEOUT_SECONDS);
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                return Err(exit_message(program, status.code(), &stderr));
            }
            Ok(None) if started.elapsed() >= deadline => {
                std::thread::spawn(move || {
                    if let Some(mut pipe) = child.stderr.take() {
                        let _ = std::io::copy(&mut pipe, &mut std::io::sink());
                    }
                    let _ = child.wait();
                });
                return Ok(());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(io_message(&err, program)),
        }
    }
}

fn io_message(err: &std::io::Error, program: &str) -> String {
    let text = err.to_string();
    let text = text.split(" (os error").next().unwrap_or(&text);
    format!("{text}: {program}")
}

fn exit_message(program: &str, code: Option<i32>, stderr: &str) -> String {
    let code = code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "signal".into());
    let stderr = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    let stderr: String = stderr.chars().take(400).collect();
    if stderr.is_empty() {
        format!("{program} exited {code}")
    } else {
        format!("{program} exited {code}: {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("robi-open-{unique}"));
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src").join("main.rs"), "fn main() {}\n").expect("write");
        root.canonicalize().expect("canonical root")
    }

    #[test]
    fn a_relative_path_renders_an_absolute_argument() {
        let root = workspace();
        let argv = argv_for(&root, "src/main.rs", None, "code {file_path}").expect("an argv");
        assert_eq!(argv[0], "code");
        assert_eq!(
            argv[1],
            root.join("src").join("main.rs").display().to_string()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_line_fills_the_placeholder() {
        let root = workspace();
        let argv =
            argv_for(&root, "src/main.rs", Some(12), "zed {file_path}:{line}").expect("argv");
        assert_eq!(
            argv[1],
            format!("{}:12", argv[1].rsplit_once(':').expect("a colon").0)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_path_outside_the_workspace_is_refused() {
        let root = workspace();
        let error = argv_for(&root, "../secret", None, "code {file_path}").unwrap_err();
        assert!(matches!(error, OpenError::Path(_)), "{error:?}");
        assert!(error.to_string().contains("outside"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_blank_path_is_refused() {
        let root = workspace();
        let error = argv_for(&root, "  ", None, "code {file_path}").unwrap_err();
        assert!(matches!(error, OpenError::Path(_)));
        assert!(error.to_string().contains("name a file"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_absolute_path_inside_the_root_resolves_here() {
        // This layer resolves what it is given; refusing an absolute path is the
        // `/open` command's rule, because it names a file *in the workspace*. The
        // opener would otherwise refuse a spelling the resolver accepts.
        let root = workspace();
        let absolute = root.join("src").join("main.rs");
        let argv = argv_for(
            &root,
            absolute.to_str().expect("utf-8"),
            None,
            "code {file_path}",
        )
        .expect("an absolute path inside the root");
        assert_eq!(argv[1], absolute.display().to_string());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_bad_template_is_refused_before_anything_starts() {
        let root = workspace();
        let error = argv_for(&root, "src/main.rs", None, "code {column}").unwrap_err();
        assert!(matches!(error, OpenError::Template(_)), "{error:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}
