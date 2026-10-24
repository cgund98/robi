//! The `HostConfig` and mounts one container is created with.
//!
//! Pure construction: no Docker handle, no I/O. Everything here is a function of
//! the resolved [`Config`] and the [`Profile`], which makes the shape testable
//! without a daemon.
//!
//! The security posture is fixed and a settings file cannot widen it. Only the
//! fields the docs call settings-backed vary: runtime, memory, cpus, pid limit,
//! and the network mode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bollard::models::{
    HostConfig, HostConfigLogConfig, Mount, MountType, RestartPolicy, RestartPolicyNameEnum,
};

use crate::agent::sandbox::profile::WriteAccess;
use crate::agent::sandbox::Profile;

use super::config::Config;

/// Where the workspace appears inside the container.
pub const WORKSPACE: &str = "/workspace";

/// The container user's home. A volume, so the package caches survive.
pub const HOME: &str = "/home/agent";

/// The volumes the caches live on, and where they mount.
///
/// A volume holds *inputs*: downloaded crates and wheels. It survives every
/// recreation, which is what makes a reinstall after a recreation cheap.
pub const CACHE_VOLUMES: &[(&str, &str)] = &[
    ("agent_home", "/home/agent"),
    ("agent_cargo_registry", "/home/agent/.cargo/registry"),
    ("agent_cargo_git", "/home/agent/.cargo/git"),
    ("agent_pip_cache", "/home/agent/.cache/pip"),
    ("agent_uv_cache", "/home/agent/.cache/uv"),
    ("agent_npm_cache", "/home/agent/.npm"),
];

/// The invoking user's UID.
///
/// `std::process` has no accessor, and the crate has no `nix` dependency, so this
/// is the same hand-rolled `extern "C"` the process launcher uses for `kill`.
pub fn uid() -> u32 {
    #[cfg(unix)]
    unsafe {
        getuid()
    }
    #[cfg(not(unix))]
    0
}

/// The invoking user's GID.
pub fn gid() -> u32 {
    #[cfg(unix)]
    unsafe {
        getgid()
    }
    #[cfg(not(unix))]
    0
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "getuid"]
    fn getuid() -> u32;
    #[link_name = "getgid"]
    fn getgid() -> u32;
}

/// The container serving one session.
///
/// Docker allows only `[a-zA-Z0-9][a-zA-Z0-9_.-]+`, so the UUID's hyphens are the
/// only punctuation and the prefix satisfies the first-character rule.
pub fn container_name(session: &str) -> String {
    format!("robi-agent-{session}")
}

/// The `HostConfig` for the sandbox container.
pub fn host_config(config: &Config, mounts: Vec<Mount>) -> HostConfig {
    HostConfig {
        runtime: Some(config.runtime.clone()),
        memory: Some(config.memory_bytes()),
        // Swap is pinned to the memory limit, so the container cannot borrow its
        // way past it.
        memory_swap: Some(config.memory_bytes()),
        nano_cpus: Some(config.nano_cpus()),
        pids_limit: Some(i64::from(config.pids_limit)),
        // The model's commands run as the host user, so they hold no effective
        // capability whatever the bounding set says: a non-root process only gains
        // one through a setuid binary or file capabilities, and
        // `no-new-privileges` blocks both.
        cap_drop: Some(vec!["ALL".to_owned()]),
        // `CHOWN` is back for one reason: Docker creates a named volume owned by
        // root, and the container runs as the host user. The one-time setup exec
        // runs as root and needs this to hand the volume over, and `cap_drop: ALL`
        // would otherwise take it away. A non-root exec never holds it effectively.
        cap_add: Some(vec!["CHOWN".to_owned()]),
        security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
        // Writable on purpose: `pip install` lands in the container's own overlay
        // layer, which is what lets a later command in the same session use it.
        // The image is untouched and the layer dies with the container.
        readonly_rootfs: Some(false),
        // `none` unless the workspace declared a service. A declared service
        // network is attached after start; an approved egress call is connected
        // for its own duration and disconnected afterwards.
        network_mode: Some(config.network_mode().to_owned()),
        // Reaps the orphans an exec leaves behind.
        init: Some(true),
        tmpfs: Some(HashMap::from([(
            "/tmp".to_owned(),
            "rw,nosuid,size=1g".to_owned(),
        )])),
        // The stream is the record. The daemon keeps no second copy, and a
        // long-lived container does not grow one.
        log_config: Some(HostConfigLogConfig {
            typ: Some("none".to_owned()),
            config: None,
        }),
        restart_policy: Some(RestartPolicy {
            name: Some(RestartPolicyNameEnum::NO),
            maximum_retry_count: None,
        }),
        mounts: Some(mounts),
        ..Default::default()
    }
}

/// The mounts for one container.
///
/// A container starts from the image, so anything the profile does not name is
/// already absent: a deny outside every mount needs no rule.
pub fn mounts(profile: &Profile, masks: &[PathBuf]) -> Vec<Mount> {
    let workspace = &profile.workspace;
    let writable = matches!(profile.write, WriteAccess::ReadWrite);
    let mut mounts = vec![bind(workspace, WORKSPACE, writable)];

    // `.git` stays readable and write-denied, so `git status` works and a commit
    // does not. A nested mount over the workspace: the longer destination wins.
    let git = workspace.join(".git");
    if git.is_dir() {
        mounts.push(bind(&git, "/workspace/.git", false));
    }

    // Paths the profile opened explicitly. Inside the workspace they are rebased
    // under `/workspace`; outside it they keep their host path so a command that
    // names one still resolves.
    for path in grants(profile) {
        let target = target_for(workspace, &path.path);
        mounts.push(bind(&path.path, &target.to_string_lossy(), path.writable));
    }

    // The floor. One read-only `/dev/null` per protected file.
    for path in masks {
        mounts.push(mask(workspace, path));
    }

    // The cache volumes, last so the nested paths sit on top of the home volume.
    for (volume, target) in CACHE_VOLUMES {
        mounts.push(volume_mount(volume, target));
    }

    mounts
}

/// A path the profile opened for read or write.
pub struct Grant {
    pub path: PathBuf,
    pub writable: bool,
}

/// The explicit grants in the profile, deduplicated.
///
/// A write grant also opens the path for read, so it is emitted once.
pub fn grants(profile: &Profile) -> Vec<Grant> {
    let mut grants: Vec<Grant> = Vec::new();
    let mut push = |path: &PathBuf, writable: bool| {
        if !path.exists() {
            return;
        }
        if let Some(existing) = grants.iter_mut().find(|grant| &grant.path == path) {
            // A write grant is the stronger of the two.
            existing.writable |= writable;
            return;
        }
        grants.push(Grant {
            path: path.clone(),
            writable,
        });
    };
    for path in &profile.extra_writes {
        push(path, true);
    }
    for path in &profile.wide_writes {
        push(path, true);
    }
    for path in &profile.protected_write_files {
        push(path, true);
    }
    for path in &profile.protected_write_dirs {
        push(path, true);
    }
    for path in &profile.wide_reads {
        push(path, false);
    }
    for path in &profile.protected_read_files {
        push(path, false);
    }
    for path in &profile.protected_read_dirs {
        push(path, false);
    }
    grants
}

/// Rebase a host path to where the container sees it.
///
/// The workspace itself rebases to exactly `/workspace`: `join("")` would produce
/// `/workspace/`, which Docker accepts but is the wrong string to hand a model or
/// put in a mount table.
pub fn target_for(workspace: &Path, path: &Path) -> PathBuf {
    match path.strip_prefix(workspace) {
        Ok(relative) if relative.as_os_str().is_empty() => PathBuf::from(WORKSPACE),
        Ok(relative) => Path::new(WORKSPACE).join(relative),
        Err(_) => path.to_path_buf(),
    }
}

fn bind(source: &Path, target: &str, writable: bool) -> Mount {
    Mount {
        typ: Some(MountType::BIND),
        source: Some(source.to_string_lossy().into_owned()),
        target: Some(target.to_owned()),
        read_only: Some(!writable),
        ..Default::default()
    }
}

/// Hide one file behind `/dev/null`.
///
/// A read-only file bind is how a container hides a path without deleting it.
fn mask(workspace: &Path, path: &Path) -> Mount {
    Mount {
        typ: Some(MountType::BIND),
        source: Some("/dev/null".to_owned()),
        target: Some(target_for(workspace, path).to_string_lossy().into_owned()),
        read_only: Some(true),
        ..Default::default()
    }
}

fn volume_mount(volume: &str, target: &str) -> Mount {
    Mount {
        typ: Some(MountType::VOLUME),
        source: Some(volume.to_owned()),
        target: Some(target.to_owned()),
        read_only: Some(false),
        ..Default::default()
    }
}

/// The environment the container starts with, and every command inherits.
///
/// The host environment is never copied. `PATH` is the image's contract: a tool
/// belongs in the image, not in a mounted host tree.
pub fn base_env() -> Vec<(String, String)> {
    let env = vec![
        ("HOME".to_owned(), HOME.to_owned()),
        ("USER".to_owned(), "agent".to_owned()),
        ("LOGNAME".to_owned(), "agent".to_owned()),
        (
            "PATH".to_owned(),
            format!("{HOME}/.local/bin:/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin"),
        ),
        ("CARGO_HOME".to_owned(), format!("{HOME}/.cargo")),
        ("RUSTUP_HOME".to_owned(), "/usr/local/rustup".to_owned()),
        ("TMPDIR".to_owned(), "/tmp".to_owned()),
        ("XDG_CACHE_HOME".to_owned(), format!("{HOME}/.cache")),
        ("PIP_CACHE_DIR".to_owned(), format!("{HOME}/.cache/pip")),
        ("UV_CACHE_DIR".to_owned(), format!("{HOME}/.cache/uv")),
        ("npm_config_cache".to_owned(), format!("{HOME}/.npm")),
        // Git must not read the user's config: the file is not mounted, and
        // without this a `git` that resolves the path fails instead of ignoring it.
        ("GIT_CONFIG_GLOBAL".to_owned(), "/dev/null".to_owned()),
        ("GIT_CONFIG_SYSTEM".to_owned(), "/dev/null".to_owned()),
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
    ];
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::sandbox::container::config::Service;
    use crate::agent::sandbox::masks::testing::scratch_dir;
    use crate::agent::sandbox::profile::NetworkMode;

    fn profile(root: &Path) -> Profile {
        Profile {
            sandboxed: true,
            write: WriteAccess::ReadWrite,
            workspace: root.to_path_buf(),
            home: PathBuf::from("/home/ada"),
            temp_dir: PathBuf::from("/tmp/robi-call"),
            cwd: root.to_path_buf(),
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

    fn targets(mounts: &[Mount]) -> Vec<String> {
        let mut targets: Vec<String> = mounts
            .iter()
            .filter_map(|mount| mount.target.clone())
            .collect();
        targets.sort();
        targets
    }

    #[test]
    fn the_workspace_is_mounted_at_its_container_path() {
        let root = Path::new("/work/app");
        let mounts = mounts(&profile(root), &[]);
        let workspace = mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some(WORKSPACE))
            .expect("workspace mount");
        assert_eq!(workspace.source.as_deref(), Some("/work/app"));
        assert_eq!(workspace.read_only, Some(false));
    }

    #[test]
    fn a_read_only_profile_mounts_the_workspace_read_only() {
        let root = Path::new("/work/app");
        let mut profile = profile(root);
        profile.write = WriteAccess::ReadOnly;
        let mounts = mounts(&profile, &[]);
        let workspace = mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some(WORKSPACE))
            .expect("workspace mount");
        assert_eq!(workspace.read_only, Some(true));
    }

    #[test]
    fn a_grant_inside_the_workspace_is_rebased() {
        let root = scratch_dir("host-");
        std::fs::create_dir_all(root.join(".robi/plans")).unwrap();
        let mut profile = profile(&root);
        profile.write = WriteAccess::ReadOnly;
        profile.extra_writes = vec![root.join(".robi/plans")];

        let mounts = mounts(&profile, &[]);
        let plans = mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some("/workspace/.robi/plans"))
            .expect("rebased grant");
        assert_eq!(plans.read_only, Some(false));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_grant_outside_the_workspace_keeps_its_host_path() {
        let root = scratch_dir("host-out-");
        std::fs::create_dir_all(&root).unwrap();
        let shared = root.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        let workspace = root.join("app");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut profile = profile(&workspace);
        profile.wide_reads = vec![shared.clone()];
        let mounts = mounts(&profile, &[]);
        let mount = mounts
            .iter()
            .find(|mount| mount.source.as_deref() == Some(shared.to_str().unwrap()))
            .expect("outside grant");
        assert_eq!(mount.target.as_deref(), Some(shared.to_str().unwrap()));
        assert_eq!(mount.read_only, Some(true));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_floor_hides_a_file_behind_dev_null() {
        let root = Path::new("/work/app");
        let masks = vec![PathBuf::from("/work/app/.env")];
        let mounts = mounts(&profile(root), &masks);
        let masked = mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some("/workspace/.env"))
            .expect("floor mount");
        assert_eq!(masked.source.as_deref(), Some("/dev/null"));
        assert_eq!(masked.read_only, Some(true));
    }

    #[test]
    fn git_is_mounted_read_only_inside_a_writable_workspace() {
        let root = scratch_dir("host-git-");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let mounts = mounts(&profile(&root), &[]);
        let git = mounts
            .iter()
            .find(|mount| mount.target.as_deref() == Some("/workspace/.git"))
            .expect("git mount");
        assert_eq!(git.read_only, Some(true));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_cache_volumes_are_mounted() {
        let mounts = mounts(&profile(Path::new("/work/app")), &[]);
        let mounted = targets(&mounts);
        for (_, target) in CACHE_VOLUMES {
            assert!(mounted.contains(&(*target).to_owned()), "missing {target}");
        }
    }

    #[test]
    fn the_security_posture_is_fixed() {
        let config = Config::default();
        let host = host_config(&config, Vec::new());
        assert_eq!(host.cap_drop, Some(vec!["ALL".to_owned()]));
        // Granted back so the one-time `chown` of the volume roots can run as root.
        // A non-root exec gains nothing from it: `no-new-privileges` blocks setuid
        // and file capabilities.
        assert_eq!(host.cap_add, Some(vec!["CHOWN".to_owned()]));
        assert_eq!(
            host.security_opt,
            Some(vec!["no-new-privileges:true".to_owned()])
        );
        assert_eq!(host.network_mode.as_deref(), Some("none"));
        assert_eq!(host.init, Some(true));
        // Writable so `pip install` works; the layer dies with the container.
        assert_eq!(host.readonly_rootfs, Some(false));
    }

    #[test]
    fn the_settings_backed_fields_come_from_the_config() {
        let config = Config {
            runtime: "runsc".to_owned(),
            memory_mb: 8192,
            cpus: 4,
            pids_limit: 1024,
            ..Config::default()
        };
        let host = host_config(&config, Vec::new());
        assert_eq!(host.runtime.as_deref(), Some("runsc"));
        assert_eq!(host.memory, Some(8192 * 1024 * 1024));
        assert_eq!(host.memory_swap, host.memory);
        assert_eq!(host.nano_cpus, Some(4_000_000_000));
        assert_eq!(host.pids_limit, Some(1024));
    }

    #[test]
    fn a_declared_service_does_not_open_the_default_bridge() {
        let config = Config {
            services: vec![Service {
                name: "postgres".to_owned(),
                port: 5432,
            }],
            ..Config::default()
        };
        let host = host_config(&config, Vec::new());
        // Attached with `connect_network` at start, so declaring a database does
        // not also hand the container outbound egress through the default bridge.
        assert_eq!(host.network_mode.as_deref(), Some("none"));
    }

    #[test]
    fn the_global_toggle_opens_the_default_bridge() {
        let config = Config {
            network_open: true,
            ..Config::default()
        };
        let host = host_config(&config, Vec::new());
        assert_eq!(host.network_mode.as_deref(), Some("bridge"));
    }

    #[test]
    fn a_sandboxed_network_means_no_interface() {
        let host = host_config(&Config::default(), Vec::new());
        assert_eq!(host.network_mode.as_deref(), Some("none"));
        // Nothing a settings file can do adds a host gateway entry.
        assert!(host.extra_hosts.is_none());
    }

    #[test]
    fn the_container_name_is_a_legal_docker_name() {
        let name = container_name("0190f3f2-1111-7222-8333-444455556666");
        assert!(name.starts_with("robi-agent-0"));
        assert!(name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-')));
    }

    #[test]
    fn the_environment_does_not_inherit_the_host() {
        let env: HashMap<String, String> = base_env().into_iter().collect();
        assert_eq!(env.get("HOME").map(String::as_str), Some(HOME));
        assert_eq!(env.get("TMPDIR").map(String::as_str), Some("/tmp"));
        assert!(!env.contains_key("PATH_HOST"));
    }
}
