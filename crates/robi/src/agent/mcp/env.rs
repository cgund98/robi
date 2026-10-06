//! The environment an MCP stdio server receives. The parent environment is not copied.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::agent::sandbox::{command_env, EnvInput};

/// The `PATH` of the user's login shell, cached for the process.
///
/// A GUI launch (`Finder`, a desktop entry) starts Robi with the minimal system
/// `PATH`, so `uvx`, `npm`, and `pyenv` are missing. Reading the login shell's
/// `PATH` recovers the directories the terminal would have. Falls back to this
/// process's `PATH` when the shell cannot be run. Blocking; call it through
/// [`crate::agent::blocking::call`].
pub fn login_path() -> String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(resolve_login_path).clone()
}

fn resolve_login_path() -> String {
    for shell in login_shells() {
        if let Some(path) = shell_path(&shell) {
            if !path.is_empty() {
                return path;
            }
        }
    }
    std::env::var("PATH").unwrap_or_default()
}

fn login_shells() -> Vec<PathBuf> {
    let mut shells = Vec::new();
    if let Some(shell) = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()) {
        shells.push(PathBuf::from(shell));
    }
    shells.push(PathBuf::from("/bin/zsh"));
    shells.push(PathBuf::from("/bin/sh"));
    shells
}

/// Run `shell -ilc 'printf %s "$PATH"'` and read the value back. An interactive
/// login shell sources the same files a terminal would. A slow shell is killed
/// after five seconds.
fn shell_path(shell: &Path) -> Option<String> {
    if !shell.is_file() {
        return None;
    }
    let mut child = Command::new(shell)
        .arg("-ilc")
        .arg("printf %s \"$PATH\"")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // A login shell may print a banner before the value. Take the last line.
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_owned)
}

/// The environment for one stdio child.
///
/// `base_path` is the `PATH` the child resolves `command` on, usually
/// [`login_path`]. `extra_path` is the `path_entries` setting, appended like the
/// shell tool appends it. The overlay is applied after the scrub, so a token the
/// user named is present and one they did not name is not.
pub fn child_env(
    workspace: &Path,
    overlay: &BTreeMap<String, String>,
    base_path: &str,
    extra_path: &str,
) -> Vec<(String, String)> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| workspace.to_path_buf());
    let temp = std::env::temp_dir();
    let mut env = command_env(&EnvInput {
        home: &home,
        workspace,
        temp_dir: &temp,
        sandboxed: false,
        parent_path: base_path,
        extra_path,
        path_prefix: "",
        lang: std::env::var("LANG").ok().as_deref(),
        user: std::env::var("USER").ok().as_deref(),
    });
    for (key, value) in overlay {
        env.retain(|(name, _)| name != key);
        env.push((key.clone(), value.clone()));
    }
    env
}

pub fn resolve_command(command: &str, env: &[(String, String)]) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(command);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let path_value = env.iter().find(|(key, _)| key == "PATH")?.1.as_str();
    for dir in path_value.split(':') {
        let candidate = std::path::Path::new(dir).join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// The `PATH` the MCP host resolves child commands on: the login shell's
/// `PATH` with `path_entries` appended. The LSP hub resolves on the same one.
pub(crate) fn resolve_path(extra_path: &str) -> String {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    crate::agent::sandbox::append_path_extra(&login_path(), &home, extra_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scrub_keeps_the_config_secret_and_drops_the_parent_key() {
        let root = std::env::temp_dir().join(format!("robi-mcp-env-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let overlay = BTreeMap::from([("GITHUB_TOKEN".into(), "from-config".into())]);
        let env = child_env(&root, &overlay, "/usr/bin", "");
        assert!(env
            .iter()
            .any(|(key, value)| key == "GITHUB_TOKEN" && value == "from-config"));
        assert!(env.iter().any(|(key, _)| key == "HOME"));
        assert!(env.iter().any(|(key, _)| key == "PATH"));
        assert!(!env.iter().any(|(key, _)| key == "OPENAI_API_KEY"));
        assert!(!env.iter().any(|(key, _)| key == "DYLD_INSERT_LIBRARIES"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_base_path_and_extra_entries_build_the_child_path() {
        let root = std::env::temp_dir().join(format!("robi-mcp-env-path-{}", std::process::id()));
        let bin = root.join("extra-bin");
        std::fs::create_dir_all(&bin).unwrap();
        let env = child_env(&root, &BTreeMap::new(), "/usr/bin", bin.to_str().unwrap());
        let path = &env.iter().find(|(key, _)| key == "PATH").unwrap().1;
        assert!(path.split(':').any(|entry| entry == "/usr/bin"));
        let canonical = bin.canonicalize().unwrap();
        assert!(path
            .split(':')
            .any(|entry| Path::new(entry) == canonical.as_path()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_login_path_is_not_empty() {
        // Never the minimal empty string: either a shell answered or this
        // process's own PATH did.
        assert!(!login_path().is_empty());
    }

    #[test]
    fn resolve_path_appends_extra_entries() {
        let root = std::env::temp_dir().join(format!("robi-mcp-resolve-{}", std::process::id()));
        let bin = root.join("extra-bin");
        std::fs::create_dir_all(&bin).unwrap();
        let path = resolve_path(bin.to_str().unwrap());
        let canonical = bin.canonicalize().unwrap();
        assert!(path
            .split(':')
            .any(|entry| Path::new(entry) == canonical.as_path()));
        assert!(path.starts_with(&login_path()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
