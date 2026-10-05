//! The OS profile around one `shell` command.
//!
//! The host builds the profile. The child cannot widen it. `robi-core` does
//! not use this module.

mod bwrap;
mod env;
mod launch;
mod profile;
mod seatbelt;

pub(crate) use env::secret_name;
pub use env::{command_env, install_path_wrappers, EnvInput};
pub use launch::run_command;
pub use profile::{
    apply_allow_read, apply_allow_write, apply_deny, classify_path, parse_rule, push_classified,
    AccessClass, NetworkMode, ParsedRule, Profile, ProfileError,
};
pub use seatbelt::seatbelt_policy;

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

/// Matches `LoopConfig::max_tool_result_bytes`, so the tool result stays under
/// the core's ceiling and still reports truncation itself.
pub const OUTPUT_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub killed: bool,
}

/// Run `command` with `profile`. A sandboxed call uses Seatbelt or bubblewrap.
/// An unsandboxed call runs `/bin/sh` directly.
pub async fn launch(
    profile: &Profile,
    command: &str,
    cancel: CancellationToken,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let owned = profile.clone();
    let command = command.to_owned();
    let argv = crate::agent::blocking::call(move || launch_argv(&owned, &command)).await??;
    run_command(launch::Request {
        argv,
        cwd: profile.cwd.clone(),
        env: profile.env.clone(),
        timeout,
        output_limit: OUTPUT_LIMIT,
        cancel,
    })
    .await
}

fn launch_argv(profile: &Profile, command: &str) -> Result<Vec<String>, String> {
    let shell = vec!["/bin/sh".to_owned(), "-c".to_owned(), command.to_owned()];
    if !profile.sandboxed {
        return Ok(shell);
    }
    if cfg!(target_os = "macos") {
        let policy = seatbelt_policy(profile);
        return Ok(vec![
            "/usr/bin/sandbox-exec".to_owned(),
            "-p".to_owned(),
            policy,
            shell[0].clone(),
            shell[1].clone(),
            shell[2].clone(),
        ]);
    }
    if cfg!(target_os = "linux") {
        return bwrap::command(profile, &shell);
    }
    Err("sandboxed shell is not available on this platform. Set unsandboxed to true to ask the user to approve an unsandboxed command.".to_owned())
}

/// Home-relative trees a sandboxed command may read so language tools resolve.
/// Parent `PATH` entries inside these trees are kept. The list is read-only.
pub const TOOLCHAIN_ROOTS: &[&str] = &[
    ".cargo/bin",
    ".rustup",
    ".local/bin",
    // Python: pyenv, and uv's managed interpreters.
    ".pyenv",
    ".local/share/uv",
    // Go: installed commands, and the SDK from `go install`.
    "go/bin",
    "sdk/go",
    // TypeScript and Node: version managers and package-manager bins.
    ".nvm",
    ".volta",
    ".fnm",
    ".local/share/fnm",
    ".bun",
    ".local/share/pnpm",
    "Library/pnpm",
];

/// Directories a sandboxed command may read so the constructed `PATH` works.
/// Each path is the canonical absolute directory, so a symlink or firmlink
/// matches the path the kernel checks.
pub fn toolchain_reads(home: &Path) -> Vec<PathBuf> {
    TOOLCHAIN_ROOTS
        .iter()
        .filter_map(|name| canonical_dir(&home.join(name)))
        .collect()
}

fn canonical_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_symlinked_toolchain_root_is_the_canonical_directory() {
        let root = std::env::temp_dir().join(format!("robi-toolchain-{}", std::process::id()));
        let home = root.join("home");
        let real = root.join("real-cargo-bin");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(home.join(".cargo")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, home.join(".cargo/bin")).unwrap();
        #[cfg(not(unix))]
        std::fs::create_dir_all(home.join(".cargo/bin")).unwrap();
        let reads = toolchain_reads(&home);
        let cargo = reads
            .iter()
            .find(|path| path.ends_with(".cargo/bin") || path.ends_with("real-cargo-bin"))
            .expect("cargo bin");
        assert_eq!(cargo, &real.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }
}
