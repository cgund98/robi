//! The OS profile around one `shell` command.
//!
//! The host builds the profile. The child cannot widen it. `robi-core` does
//! not use this module.

mod bwrap;
mod env;
mod launch;
mod profile;
mod seatbelt;

pub use env::{command_env, EnvInput};
pub use launch::run_command;
pub use profile::{
    apply_allow_read, apply_deny, classify_path, parse_rule, push_classified, AccessClass,
    NetworkMode, ParsedRule, Profile, ProfileError,
};
pub use seatbelt::seatbelt_policy;

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

pub const TIMEOUT: Duration = Duration::from_secs(120);
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
) -> Result<CommandOutput, String> {
    let argv = launch_argv(profile, command)?;
    run_command(launch::Request {
        argv,
        cwd: profile.cwd.clone(),
        env: profile.env.clone(),
        timeout: TIMEOUT,
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

/// Directories a sandboxed command may read so the constructed `PATH` works.
pub fn toolchain_reads(home: &Path) -> Vec<PathBuf> {
    [".cargo/bin", ".rustup", ".local/bin"]
        .into_iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_dir())
        .collect()
}
