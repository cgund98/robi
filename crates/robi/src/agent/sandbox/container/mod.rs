//! `shell` inside a gVisor container, managed over the Docker API.
//!
//! One container serves one session. It is created on the first sandboxed
//! command, reused by every later command in that session, and removed when the
//! session ends. Preserving it is the point: `pip install` lands in the
//! container's own layer, so the next command in the same session sees it.
//!
//! Nothing here spawns `docker`. Every call is `bollard` over the daemon socket.
//!
//! The two submodules hold the parts that do not need a daemon:
//!
//! - [`config`] resolves the settings layers.
//! - [`host`] builds the `HostConfig` and its mounts.
//!
//! Egress is per call and approval-gated. A command approved with `network:
//! "unrestricted"` is connected to [`EGRESS_NETWORK`] for its own duration and
//! disconnected afterwards; there is no workspace-wide network switch. A service
//! that only listens on the host is reached the same way, or with
//! `unsandboxed: true`.

pub mod config;
pub mod host;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bollard::container::LogOutput;
use bollard::exec::CreateExecOptions;
use bollard::exec::StartExecResults;
use bollard::models::{
    ContainerCreateBody, NetworkConnectRequest, NetworkCreateRequest, NetworkDisconnectRequest,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, KillContainerOptionsBuilder,
    RemoveContainerOptionsBuilder,
};
use bollard::Docker;
use futures_util::StreamExt;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::agent::sandbox::masks;
use crate::agent::sandbox::profile::NetworkMode;
use crate::agent::sandbox::{CommandOutput, Profile, OUTPUT_LIMIT};
use robi_core::ids::SessionId;

use self::config::{Config, ConfigError};

/// The bridge an approved `network: "unrestricted"` call is attached to.
pub const EGRESS_NETWORK: &str = "robi-egress";

/// The labels every sandbox container carries, so the reaper can find orphans.
pub const LABEL: &str = "robi.sandbox";
pub const LABEL_SESSION: &str = "robi.session";

/// Upper bound on floor mounts. Past it, container creation is slow enough that
/// the workspace is almost certainly misconfigured.
pub const MAX_MASKS: usize = 2_000;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("the Docker daemon is not reachable: {0}")]
    Daemon(#[from] bollard::errors::Error),
    #[error(
        "the runtime `{0}` is not registered with the Docker daemon. \
         Register it, or set sandbox_mechanism to os"
    )]
    RuntimeMissing(String),
    #[error("the image `{0}` is missing and could not be pulled: {1}")]
    ImageMissing(String, String),
    #[error("the workspace {0} is not a directory")]
    Workspace(PathBuf),
    #[error("the workspace needs {0} path masks, above the {MAX_MASKS} limit")]
    TooManyMasks(usize),
    #[error("the workspace is too large to hide secret files")]
    WorkspaceTooLarge,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("container setup failed: {0}")]
    Setup(String),
}

/// What the sandbox remembers between calls.
struct State {
    name: String,
    masks: Vec<PathBuf>,
    config: Config,
    /// Where the sandbox dials a declared service: its DNS name on
    /// `service_network`. `None` when the workspace declared no service.
    service_host: Option<String>,
    /// Set when the container must be recreated, because its masks, its config,
    /// or its liveness changed.
    dirty: bool,
}

/// The container serving one session.
pub struct Sandbox {
    docker: Arc<Docker>,
    session: SessionId,
    profile: Profile,
    uid: u32,
    gid: u32,
    state: Mutex<State>,
}

impl Sandbox {
    /// Creates or attaches to the session's container.
    pub async fn new(
        profile: &Profile,
        session: SessionId,
        config: Config,
    ) -> Result<Self, SandboxError> {
        let docker = Arc::new(Docker::connect_with_local_defaults()?);
        preflight(&docker, &config).await?;

        let workspace = profile
            .workspace
            .canonicalize()
            .map_err(|_| SandboxError::Workspace(profile.workspace.clone()))?;
        let masks = floor_masks(profile.clone()).await?;
        if masks.len() > MAX_MASKS {
            return Err(SandboxError::TooManyMasks(masks.len()));
        }

        let sandbox = Self {
            docker,
            session,
            profile: Profile {
                workspace,
                ..profile.clone()
            },
            uid: host::uid(),
            gid: host::gid(),
            state: Mutex::new(State {
                name: host::container_name(&session.to_string()),
                masks,
                config,
                service_host: None,
                dirty: true,
            }),
        };
        sandbox.ensure_ready().await?;
        Ok(sandbox)
    }

    /// Runs one command.
    ///
    /// The container is reused unless the mask set or the resolved config
    /// changed, because both are create-time properties.
    pub async fn exec(
        &self,
        command: &str,
        timeout: Duration,
        cancel: CancellationToken,
        config: Config,
        network: NetworkMode,
    ) -> Result<CommandOutput, SandboxError> {
        // Serialised: the container, its mounts, and its network attachment are
        // per session, and the `shell` tool is exclusive.
        let mut state = self.state.lock().await;
        if config != state.config {
            state.config = config;
            state.dirty = true;
        }
        self.ensure_ready_locked(&mut state).await?;

        // An approved `network: "unrestricted"` call gets an interface for its own
        // duration. A container cannot change `network_mode` after creation, but
        // it can be attached to a bridge and detached again. When the global
        // toggle already opened the network, the default bridge provides it and
        // there is nothing to attach.
        let egress = matches!(network, NetworkMode::Unrestricted) && !state.config.egress_open();
        if egress {
            self.connect(&state.name, EGRESS_NETWORK).await?;
        }

        let env = config::command_env(&host::base_env(), &state.config);
        // The profile's `cwd` is a host path, guaranteed inside the workspace.
        // Rebase it, so `cwd: "crates/robi"` runs in `/workspace/crates/robi`
        // instead of silently running in `/workspace`.
        let cwd = container_cwd(&self.profile);
        let exec = self
            .docker
            .create_exec(
                &state.name,
                CreateExecOptions {
                    cmd: Some(vec![
                        "/bin/sh".to_owned(),
                        "-c".to_owned(),
                        command.to_owned(),
                    ]),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    tty: Some(false),
                    working_dir: Some(cwd.clone()),
                    user: Some(format!("{}:{}", self.uid, self.gid)),
                    env: Some(env),
                    ..Default::default()
                },
            )
            .await?;

        let StartExecResults::Attached { mut output, .. } =
            self.docker.start_exec(&exec.id, None).await?
        else {
            return Err(SandboxError::Setup("exec did not attach".to_owned()));
        };

        let mut buffer = StreamBuffer::new(OUTPUT_LIMIT / 2);
        let mut killed = false;
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    killed = true;
                    break;
                }
                _ = &mut deadline => {
                    killed = true;
                    break;
                }
                frame = output.next() => match frame {
                    Some(Ok(frame)) => buffer.push(frame),
                    Some(Err(err)) => return Err(SandboxError::Daemon(err)),
                    None => break,
                },
            }
        }

        if killed {
            // An exec has no cancel API, so the container is the unit. Draining
            // keeps the socket from erroring under the next call.
            let _ = self
                .docker
                .kill_container(
                    &state.name,
                    Some(
                        KillContainerOptionsBuilder::default()
                            .signal("SIGKILL")
                            .build(),
                    ),
                )
                .await;
            while let Some(Ok(frame)) = output.next().await {
                buffer.push(frame);
            }
            state.dirty = true;
        }

        // Egress is for this call only. Leaving the interface attached would give
        // the next command a network it was never approved for.
        if egress {
            self.disconnect_quietly(&state.name, EGRESS_NETWORK).await;
        }

        let exit_code = if killed {
            -1
        } else {
            self.docker
                .inspect_exec(&exec.id)
                .await
                .ok()
                .and_then(|info| info.exit_code)
                .unwrap_or(-1) as i32
        };

        let (stdout, mut stderr, truncated) = buffer.finish();
        if killed {
            if !stderr.is_empty() && !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            stderr.push_str("command was killed");
        }
        Ok(CommandOutput {
            exit_code,
            stdout,
            stderr,
            truncated,
            killed,
            cwd: Some(cwd),
        })
    }

    /// Removes the container. The workspace and the cache volumes are left alone.
    pub async fn shutdown(&self) {
        let state = self.state.lock().await;
        let _ = self
            .docker
            .remove_container(
                &state.name,
                Some(RemoveContainerOptionsBuilder::default().force(true).build()),
            )
            .await;
    }

    async fn ensure_ready(&self) -> Result<(), SandboxError> {
        let mut state = self.state.lock().await;
        self.ensure_ready_locked(&mut state).await
    }

    async fn ensure_ready_locked(&self, state: &mut State) -> Result<(), SandboxError> {
        // The walk is filesystem work; keep it off the async worker.
        let fresh = floor_masks(self.profile.clone()).await?;
        if fresh != state.masks {
            // A new secret file invalidates the mounts, so the container goes.
            state.masks = fresh;
            state.dirty = true;
        }

        let running = match self.docker.inspect_container(&state.name, None).await {
            Ok(info) => info
                .state
                .and_then(|container| container.running)
                .unwrap_or(false),
            Err(err) if is_not_found(&err) => false,
            Err(err) => return Err(SandboxError::Daemon(err)),
        };

        if running && !state.dirty {
            return Ok(());
        }
        self.remove(&state.name).await;
        self.create(state).await
    }

    async fn create(&self, state: &mut State) -> Result<(), SandboxError> {
        let mounts = host::mounts(&self.profile, &state.masks);
        self.docker
            .create_container(
                Some(
                    CreateContainerOptionsBuilder::default()
                        .name(&state.name)
                        .build(),
                ),
                ContainerCreateBody {
                    image: Some(state.config.image.clone()),
                    // Holds the mounts and the namespaces; every command arrives
                    // through `exec`.
                    cmd: Some(vec!["sleep".to_owned(), "infinity".to_owned()]),
                    user: Some(format!("{}:{}", self.uid, self.gid)),
                    // The mount root, so a command that somehow arrives without an
                    // exec `working_dir` still starts at the workspace.
                    working_dir: Some(host::WORKSPACE.to_owned()),
                    env: Some(
                        host::base_env()
                            .into_iter()
                            .map(|(key, value)| format!("{key}={value}"))
                            .collect(),
                    ),
                    host_config: Some(host::host_config(&state.config, mounts)),
                    labels: Some(HashMap::from([
                        (LABEL.to_owned(), "container".to_owned()),
                        (LABEL_SESSION.to_owned(), self.session.to_string()),
                        (
                            "robi.workspace".to_owned(),
                            self.profile.workspace.to_string_lossy().into_owned(),
                        ),
                    ])),
                    ..Default::default()
                },
            )
            .await?;
        self.docker.start_container(&state.name, None).await?;

        // The drop guard: if anything below fails, the half-built container is
        // not left behind for the next call to reuse.
        if let Err(err) = self.attach_services(state).await {
            self.remove(&state.name).await;
            return Err(err);
        }
        if let Err(err) = self.prepare_home(&state.name).await {
            self.remove(&state.name).await;
            return Err(err);
        }

        state.dirty = false;
        Ok(())
    }

    /// Joins the network that carries the workspace's declared services.
    ///
    /// Reaching one needs a Docker network: the container resolves the service
    /// name there. A service that only listens on the host is not reachable this
    /// way, and `config::resolve` refuses the declaration rather than starting a
    /// container that cannot connect.
    async fn attach_services(&self, state: &mut State) -> Result<(), SandboxError> {
        let Some(network) = state.config.service_network.clone() else {
            return Ok(());
        };
        if state.config.services.is_empty() {
            return Ok(());
        }
        self.connect(&state.name, &network).await?;
        // Docker's embedded DNS resolves the service name on that network.
        state.service_host = state
            .config
            .services
            .first()
            .map(|service| service.name.clone());
        Ok(())
    }

    async fn disconnect_quietly(&self, container: &str, network: &str) {
        let _ = self
            .docker
            .disconnect_network(
                network,
                NetworkDisconnectRequest {
                    container: container.to_owned(),
                    force: Some(true),
                },
            )
            .await;
    }

    async fn connect(&self, container: &str, network: &str) -> Result<(), SandboxError> {
        // A user-defined network must exist; the workspace names an existing one.
        // `robi-egress` is the harness's own and is created on first use.
        if network == EGRESS_NETWORK {
            ensure_network(&self.docker, network).await?;
        }
        self.docker
            .connect_network(
                network,
                NetworkConnectRequest {
                    container: container.to_owned(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|err| SandboxError::Setup(format!("connect {network}: {err}")))
    }

    /// Docker creates a volume owned by root, and the container runs as the host
    /// user. One preparation exec hands the home volume over.
    ///
    /// The workspace is deliberately outside it: it is the user's directory.
    ///
    /// Takes the name rather than locking state: the caller already holds that
    /// lock, and re-entering it would deadlock.
    async fn prepare_home(&self, name: &str) -> Result<(), SandboxError> {
        self.run_setup(name, &chown_script(self.uid, self.gid))
            .await
    }

    /// Runs one setup command as root and waits for it.
    ///
    /// The output is kept for the error: a failed setup step is a sandbox that
    /// cannot start, and `exited 1` on its own does not say why.
    async fn run_setup(&self, name: &str, command: &str) -> Result<(), SandboxError> {
        let exec = self
            .docker
            .create_exec(
                name,
                CreateExecOptions {
                    cmd: Some(vec![
                        "/bin/sh".to_owned(),
                        "-c".to_owned(),
                        command.to_owned(),
                    ]),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    user: Some("0:0".to_owned()),
                    ..Default::default()
                },
            )
            .await?;
        let mut collected = String::new();
        if let StartExecResults::Attached { mut output, .. } =
            self.docker.start_exec(&exec.id, None).await?
        {
            // Drain so the setup command is not killed by a closed socket.
            while let Some(chunk) = output.next().await {
                match chunk {
                    Ok(chunk) => {
                        collected.push_str(&String::from_utf8_lossy(chunk.as_ref()));
                    }
                    Err(err) => return Err(SandboxError::Daemon(err)),
                }
            }
        }
        let mut delay = Duration::from_millis(10);
        for _ in 0..200 {
            let info = self.docker.inspect_exec(&exec.id).await?;
            if !info.running.unwrap_or(false) {
                let code = info.exit_code.unwrap_or(-1);
                return if code == 0 {
                    Ok(())
                } else {
                    Err(SandboxError::Setup(format!(
                        "setup command exited {code}: {}{}",
                        first_lines(&collected, 8),
                        hint_for_setup_failure(&collected)
                    )))
                };
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_millis(200));
        }
        Err(SandboxError::Setup(
            "setup command did not finish".to_owned(),
        ))
    }

    async fn remove(&self, name: &str) {
        let _ = self
            .docker
            .remove_container(
                name,
                Some(RemoveContainerOptionsBuilder::default().force(true).build()),
            )
            .await;
    }
}

/// Where a command runs, in container terms.
///
/// The default is the workspace root, so a command that names no `cwd` never needs
/// a `cd`. A `cwd` is rebased through the same helper the mounts use, so it agrees
/// with where the directory actually is. A directory that has gone missing falls
/// back to the root rather than failing the exec, which is what a stale `cwd` on a
/// fresh checkout looks like.
fn container_cwd(profile: &Profile) -> String {
    if !profile.cwd.is_dir() {
        return host::WORKSPACE.to_owned();
    }
    host::target_for(&profile.workspace, &profile.cwd)
        .to_string_lossy()
        .into_owned()
}

/// The shell script that hands the cache volumes to the host user.
///
/// Docker creates a named volume owned by root, and copies the image's ownership
/// into it on first use. The container runs as the host user, so each volume root
/// must be reassigned once.
///
/// A root whose owner already matches is skipped: on a recreation the volumes are
/// already correct, and re-walking a populated cargo registry would cost seconds
/// per container.
fn chown_script(uid: u32, gid: u32) -> String {
    let roots = host::CACHE_VOLUMES
        .iter()
        .map(|(_, target)| *target)
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        r#"set -e
for dir in {roots}; do
  [ -d "$dir" ] || continue
  owner=$(stat -c %u "$dir" 2>/dev/null || echo unknown)
  [ "$owner" = "{uid}" ] && continue
  chown -R {uid}:{gid} "$dir"
done"#
    )
}

/// The first `n` non-empty lines of a command's output.
fn first_lines(output: &str, n: usize) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(n)
        .collect();
    if lines.is_empty() {
        "no output".to_owned()
    } else {
        lines.join("; ")
    }
}

/// A next step for the setup failures worth explaining.
///
/// `chown` returning `Operation not permitted` means the container lost
/// `CAP_CHOWN`, which is what happens if someone tightens the capability set
/// again. The rest speak for themselves.
fn hint_for_setup_failure(output: &str) -> String {
    let lower = output.to_ascii_lowercase();
    if lower.contains("operation not permitted") {
        ". The container needs CAP_CHOWN for this: it is granted back after \
         `cap_drop: [\"ALL\"]` in `host::host_config`"
            .to_owned()
    } else if lower.contains("no such file or directory") {
        ". The image does not have this path; check `docker/agent.Dockerfile`".to_owned()
    } else {
        String::new()
    }
}

/// Every protected file in the workspace, on the blocking pool.
async fn floor_masks(profile: Profile) -> Result<Vec<PathBuf>, SandboxError> {
    crate::agent::blocking::call(move || masks::floor_files(&profile))
        .await
        .map_err(SandboxError::Setup)?
        .map_err(|err| {
            if err.contains("too large") {
                SandboxError::WorkspaceTooLarge
            } else {
                SandboxError::Setup(err)
            }
        })
}

/// Fails before a command runs if the daemon cannot host the sandbox.
async fn preflight(docker: &Docker, config: &Config) -> Result<(), SandboxError> {
    let info = docker.info().await?;
    let registered = info
        .runtimes
        .as_ref()
        .is_some_and(|runtimes| runtimes.contains_key(&config.runtime));
    if !registered {
        return Err(SandboxError::RuntimeMissing(config.runtime.clone()));
    }
    ensure_image(docker, &config.image).await
}

/// Pulls `image` when the daemon does not have it.
async fn ensure_image(docker: &Docker, image: &str) -> Result<(), SandboxError> {
    if docker.inspect_image(image).await.is_ok() {
        return Ok(());
    }
    let options = CreateImageOptionsBuilder::default()
        .from_image(image)
        .build();
    let mut pull = docker.create_image(Some(options), None, None);
    while let Some(chunk) = pull.next().await {
        chunk.map_err(|err| SandboxError::ImageMissing(image.to_owned(), err.to_string()))?;
    }
    Ok(())
}

/// Creates `network` when the daemon does not have it.
///
/// Only the harness's own networks are created; a workspace's `service_network`
/// must already exist, so a typo fails instead of silently starting an empty
/// network the service is not on.
async fn ensure_network(docker: &Docker, network: &str) -> Result<(), SandboxError> {
    if docker.inspect_network(network, None).await.is_ok() {
        return Ok(());
    }
    docker
        .create_network(NetworkCreateRequest {
            name: network.to_owned(),
            driver: Some("bridge".to_owned()),
            ..Default::default()
        })
        .await
        .map_err(|err| SandboxError::Setup(format!("create network {network}: {err}")))?;
    Ok(())
}

fn is_not_found(err: &bollard::errors::Error) -> bool {
    matches!(
        err,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

/// Collects one multiplexed exec stream into two capped buffers.
///
/// The frames arrive interleaved on one stream, tagged with their stream. Bytes
/// past the cap are read and dropped so the socket keeps draining: a blocked exec
/// would never exit.
struct StreamBuffer {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    limit: usize,
    truncated: bool,
}

impl StreamBuffer {
    fn new(limit: usize) -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            limit,
            truncated: false,
        }
    }

    fn push(&mut self, frame: LogOutput) {
        let (buffer, bytes) = match frame {
            LogOutput::StdOut { message } | LogOutput::Console { message } => {
                (&mut self.stdout, message)
            }
            LogOutput::StdErr { message } => (&mut self.stderr, message),
            LogOutput::StdIn { .. } => return,
        };
        let room = self.limit.saturating_sub(buffer.len());
        if bytes.len() > room {
            self.truncated = true;
        }
        buffer.extend_from_slice(&bytes[..bytes.len().min(room)]);
    }

    fn finish(self) -> (String, String, bool) {
        (
            String::from_utf8_lossy(&self.stdout).into_owned(),
            String::from_utf8_lossy(&self.stderr).into_owned(),
            self.truncated,
        )
    }
}

/// The session's sandbox, created on first use.
///
/// A map rather than one sandbox per call: the container is preserved for the
/// session, so the second `shell` call reuses it.
#[derive(Default)]
pub struct Registry {
    sandboxes: Mutex<HashMap<SessionId, Arc<Sandbox>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide registry.
    ///
    /// One daemon, one process, one map keyed by session: sharing it here keeps
    /// the sandbox out of every `AgentFactory` and `ToolContext` literal, and the
    /// container survives the per-turn rebuild of the tool registry. Tests that
    /// need isolation build their own with [`Registry::new`].
    pub fn global() -> &'static Registry {
        static GLOBAL: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
        GLOBAL.get_or_init(Registry::new)
    }

    /// The session's sandbox, creating the container if this is the first call.
    ///
    /// The config is compared in [`Sandbox::exec`] rather than here, so one place
    /// decides whether the container is recreated.
    pub async fn get(
        &self,
        profile: &Profile,
        session: SessionId,
        config: Config,
    ) -> Result<Arc<Sandbox>, SandboxError> {
        let mut map = self.sandboxes.lock().await;
        if let Some(sandbox) = map.get(&session) {
            return Ok(Arc::clone(sandbox));
        }
        let sandbox = Arc::new(Sandbox::new(profile, session, config).await?);
        map.insert(session, Arc::clone(&sandbox));
        Ok(sandbox)
    }

    /// Removes the session's container and drops the entry.
    pub async fn release(&self, session: SessionId) {
        let sandbox = self.sandboxes.lock().await.remove(&session);
        if let Some(sandbox) = sandbox {
            sandbox.shutdown().await;
        }
    }
}

/// Runs one command in the session's container.
pub async fn launch(
    registry: &Registry,
    profile: &Profile,
    command: &str,
    cancel: CancellationToken,
    timeout: Duration,
    session: SessionId,
    config: Config,
) -> Result<CommandOutput, SandboxError> {
    if command.trim().is_empty() {
        return Err(SandboxError::Setup("command is required".to_owned()));
    }
    let sandbox = registry.get(profile, session, config.clone()).await?;
    sandbox
        .exec(command, timeout, cancel, config, profile.network)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_stdout(bytes: &[u8]) -> LogOutput {
        LogOutput::StdOut {
            message: bytes.to_vec().into(),
        }
    }

    fn frame_stderr(bytes: &[u8]) -> LogOutput {
        LogOutput::StdErr {
            message: bytes.to_vec().into(),
        }
    }

    #[test]
    fn the_two_streams_are_kept_apart() {
        let mut buffer = StreamBuffer::new(1024);
        buffer.push(frame_stdout(b"out\n"));
        buffer.push(frame_stderr(b"err\n"));
        let (stdout, stderr, truncated) = buffer.finish();
        assert_eq!(stdout, "out\n");
        assert_eq!(stderr, "err\n");
        assert!(!truncated);
    }

    #[test]
    fn bytes_past_the_cap_are_dropped_and_flagged() {
        let mut buffer = StreamBuffer::new(4);
        buffer.push(frame_stdout(b"abcdefgh"));
        let (stdout, _, truncated) = buffer.finish();
        assert_eq!(stdout, "abcd");
        assert!(truncated);
    }

    #[test]
    fn each_stream_gets_its_own_cap() {
        let mut buffer = StreamBuffer::new(4);
        buffer.push(frame_stdout(b"abcd"));
        buffer.push(frame_stderr(b"abcd"));
        let (stdout, stderr, truncated) = buffer.finish();
        assert_eq!(stdout, "abcd");
        assert_eq!(stderr, "abcd");
        // Neither stream exceeded its own cap.
        assert!(!truncated);
    }

    #[test]
    fn a_frame_crossing_the_cap_is_cut_at_the_cap() {
        let mut buffer = StreamBuffer::new(4);
        buffer.push(frame_stdout(b"ab"));
        buffer.push(frame_stdout(b"cdef"));
        let (stdout, _, truncated) = buffer.finish();
        assert_eq!(stdout, "abcd");
        assert!(truncated);
    }

    #[test]
    fn stdin_frames_are_ignored() {
        let mut buffer = StreamBuffer::new(64);
        buffer.push(LogOutput::StdIn {
            message: b"typed".to_vec().into(),
        });
        let (stdout, stderr, truncated) = buffer.finish();
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn invalid_utf8_does_not_panic() {
        let mut buffer = StreamBuffer::new(64);
        buffer.push(frame_stdout(&[0xff, 0xfe]));
        let (stdout, _, _) = buffer.finish();
        assert!(!stdout.is_empty());
    }

    #[test]
    fn the_default_mechanism_is_the_os_sandbox() {
        // The container is opt-in: a workspace that sets nothing keeps
        // bubblewrap or Seatbelt.
        assert_eq!(Config::default().mechanism, config::Mechanism::Os);
    }

    /// A profile whose `cwd` is `sub` inside `root`.
    fn profile_at(
        root: &std::path::Path,
        cwd: std::path::PathBuf,
    ) -> crate::agent::sandbox::Profile {
        crate::agent::sandbox::Profile {
            sandboxed: true,
            write: crate::agent::sandbox::WriteAccess::ReadWrite,
            workspace: root.to_path_buf(),
            home: std::path::PathBuf::from("/home/ada"),
            temp_dir: std::path::PathBuf::from("/tmp/robi-call"),
            cwd,
            network: NetworkMode::Deny,
            env: Vec::new(),
            toolchain_reads: Vec::new(),
            wide_reads: Vec::new(),
            wide_writes: Vec::new(),
            extra_writes: Vec::new(),
            protected_read_files: Vec::new(),
            protected_write_files: Vec::new(),
            protected_read_dirs: Vec::new(),
            protected_write_dirs: Vec::new(),
            deny_reads: Vec::new(),
            deny_writes: Vec::new(),
        }
    }

    #[test]
    fn the_default_working_directory_is_the_workspace_mount() {
        // No `cwd` given: the command starts at the mount root, so it never needs
        // a `cd /workspace`.
        let root = crate::agent::sandbox::masks::testing::scratch_dir("container-cwd-default");
        let profile = profile_at(&root, root.clone());
        assert_eq!(container_cwd(&profile), "/workspace");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_subdirectory_cwd_is_rebased_under_the_mount() {
        // The bug this replaced: `cwd` was accepted and then ignored, so every
        // command ran at the root and a `cd` was the only way down.
        let root = crate::agent::sandbox::masks::testing::scratch_dir("container-cwd-sub");
        let sub = root.join("crates/robi");
        std::fs::create_dir_all(&sub).unwrap();
        let profile = profile_at(&root, sub);
        assert_eq!(container_cwd(&profile), "/workspace/crates/robi");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_cwd_falls_back_to_the_mount_root() {
        // A stale `cwd` after a branch switch should not fail the exec.
        let root = crate::agent::sandbox::masks::testing::scratch_dir("container-cwd-gone");
        let profile = profile_at(&root, root.join("deleted"));
        assert_eq!(container_cwd(&profile), "/workspace");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_chown_script_targets_the_host_user_and_every_volume() {
        let script = chown_script(502, 20);
        assert!(script.contains("chown -R 502:20"));
        for (_, target) in crate::agent::sandbox::container::host::CACHE_VOLUMES {
            assert!(script.contains(target), "missing {target}");
        }
        // `set -e` so a failed chown fails the exec instead of being skipped.
        assert!(script.starts_with("set -e"));
    }

    #[test]
    fn the_chown_script_skips_a_root_that_is_already_correct() {
        // A recreation must not re-walk a populated cargo registry.
        let script = chown_script(502, 20);
        assert!(script.contains(r#"[ "$owner" = "502" ] && continue"#));
    }

    #[test]
    fn a_setup_failure_keeps_the_command_output() {
        let output = "chown: /home/agent: Operation not permitted\n\n";
        assert_eq!(
            first_lines(output, 8),
            "chown: /home/agent: Operation not permitted"
        );
        // And it names the likely cause, which is what was missing before.
        assert!(hint_for_setup_failure(output).contains("CAP_CHOWN"));
    }

    #[test]
    fn a_setup_failure_with_no_output_still_says_something() {
        assert_eq!(first_lines("", 8), "no output");
        assert_eq!(hint_for_setup_failure(""), "");
    }

    #[test]
    fn the_mask_limit_is_below_the_walk_limit() {
        // A workspace the walk refuses is a different failure from one with too
        // many protected files; the two bounds should not cross.
        const MASK_LIMIT: usize = MAX_MASKS;
        const WALK_LIMIT: usize = crate::agent::sandbox::masks::MAX_VISITED;
        const _: () = assert!(MASK_LIMIT < WALK_LIMIT);
    }
}
