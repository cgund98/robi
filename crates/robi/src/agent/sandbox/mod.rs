//! The OS profile around one `shell` command.
//!
//! The host builds the profile. The child cannot widen it. `robi-core` does
//! not use this module.
//!
//! Two mechanisms run that profile. [`Mechanism::Os`] is Seatbelt on macOS and
//! bubblewrap on Linux, and it is the default. [`Mechanism::Container`] is a
//! gVisor container over the Docker API, available on both platforms. Neither
//! replaces the other.

mod bwrap;
#[cfg(feature = "container-sandbox")]
pub mod container;
mod env;
mod launch;
pub mod masks;
mod profile;
mod seatbelt;

pub(crate) use env::append_path_extra;
pub(crate) use env::secret_name;
pub use env::{command_env, install_path_wrappers, EnvInput};
pub use launch::run_command;
pub use profile::{
    apply_allow_read, apply_allow_write, apply_deny, classify_path, parse_rule, push_classified,
    AccessClass, NetworkMode, ParsedRule, Profile, ProfileError, WriteAccess,
};
pub use seatbelt::seatbelt_policy;

/// Which sandbox runs a command.
///
/// Re-exported from the container module when the feature is on, so the setting
/// has one definition. Without the feature the enum still exists, with only
/// [`Mechanism::Os`] reachable: choosing `container` is then refused.
#[cfg(feature = "container-sandbox")]
pub use container::config::Mechanism;

#[cfg(not(feature = "container-sandbox"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mechanism {
    #[default]
    Os,
    Container,
}

use std::path::{Path, PathBuf};
use std::time::Duration;

use robi_core::ids::SessionId;
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

/// Everything one call needs beyond the profile.
pub struct Launch<'a> {
    pub profile: &'a Profile,
    pub command: &'a str,
    pub cancel: CancellationToken,
    pub timeout: Duration,
    pub session: SessionId,
    pub mechanism: Mechanism,
    /// Live settings, for the container mechanism's own keys.
    pub settings: Option<&'a dyn crate::domain::settings::store::SettingsStore>,
    /// Whether the workspace acknowledged its `sandbox.toml`.
    pub project_trusted: bool,
    /// The session's containers. Present only in a build that has them.
    #[cfg(feature = "container-sandbox")]
    pub registry: Option<&'a container::Registry>,
}

impl<'a> Launch<'a> {
    pub fn new(
        profile: &'a Profile,
        command: &'a str,
        cancel: CancellationToken,
        timeout: Duration,
        session: SessionId,
    ) -> Self {
        Self {
            profile,
            command,
            cancel,
            timeout,
            session,
            mechanism: Mechanism::Os,
            settings: None,
            project_trusted: false,
            #[cfg(feature = "container-sandbox")]
            registry: None,
        }
    }

    pub fn with_mechanism(mut self, mechanism: Mechanism) -> Self {
        self.mechanism = mechanism;
        self
    }

    /// Attaches the settings the container mechanism resolves from.
    pub fn with_settings(
        mut self,
        settings: Option<&'a dyn crate::domain::settings::store::SettingsStore>,
        project_trusted: bool,
    ) -> Self {
        self.settings = settings;
        self.project_trusted = project_trusted;
        self
    }

    /// Attaches the session registry the container mechanism reuses.
    #[cfg(feature = "container-sandbox")]
    pub fn with_registry(mut self, registry: &'a container::Registry) -> Self {
        self.registry = Some(registry);
        self
    }
}

/// Run `command` with `profile` under the selected mechanism.
///
/// The `os` mechanism is a process, so it needs no session. The container
/// mechanism is per session, so it takes the id it was built with.
pub async fn launch(request: Launch<'_>) -> Result<CommandOutput, String> {
    let profile = request.profile;
    if !profile.sandboxed {
        return launch_host(profile, request.command, request.cancel, request.timeout).await;
    }
    match request.mechanism {
        Mechanism::Os => launch_os(profile, request.command, request.cancel, request.timeout).await,
        Mechanism::Container => launch_container(request).await,
    }
}

/// `/bin/sh` on the host, with no profile. The approved escape hatch.
async fn launch_host(
    profile: &Profile,
    command: &str,
    cancel: CancellationToken,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    run_command(launch::Request {
        argv: vec!["/bin/sh".to_owned(), "-c".to_owned(), command.to_owned()],
        cwd: profile.cwd.clone(),
        env: profile.env.clone(),
        timeout,
        output_limit: OUTPUT_LIMIT,
        cancel,
    })
    .await
}

/// Seatbelt or bubblewrap. The default, and unchanged from what shipped.
async fn launch_os(
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

#[cfg(feature = "container-sandbox")]
async fn launch_container(request: Launch<'_>) -> Result<CommandOutput, String> {
    let Some(registry) = request.registry else {
        return Err(
            "sandbox could not be applied: the container sandbox needs a session registry"
                .to_owned(),
        );
    };
    let config = container::config::resolve(
        request.settings,
        &request.profile.workspace,
        request.project_trusted,
    )
    .await
    .map_err(|err| format!("sandbox could not be applied: {err}"))?;
    // A workspace that keeps the `os` mechanism must not pay for the container:
    // resolving the config is cheap, but only this branch asks for the daemon.
    if config.mechanism == Mechanism::Os {
        return launch_os(
            request.profile,
            request.command,
            request.cancel,
            request.timeout,
        )
        .await;
    }
    container::launch(
        registry,
        request.profile,
        request.command,
        request.cancel,
        request.timeout,
        request.session,
        config,
    )
    .await
    .map_err(|err| format!("sandbox could not be applied: {err}"))
}

#[cfg(not(feature = "container-sandbox"))]
async fn launch_container(_request: Launch<'_>) -> Result<CommandOutput, String> {
    // This build has no container sandbox. Refuse rather than run the command on
    // the host: a setting the user chose must not become a silent bypass.
    Err(
        "sandbox could not be applied: this build has no container sandbox. \
         Rebuild with `--features container-sandbox`, or set sandbox_mechanism to os."
            .to_owned(),
    )
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
