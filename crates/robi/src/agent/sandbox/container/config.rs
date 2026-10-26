//! The sandbox settings, resolved for one workspace.
//!
//! Three layers, narrowest last: the built-in defaults, the global settings in
//! `~/.robi/config.toml`, and `<workspace>/.robi/sandbox.toml` when the user has
//! acknowledged it. A field the workspace file omits keeps the global value, so a
//! project that raises `memory_mb` still inherits the machine's `image`.
//!
//! No I/O beyond one file read, and no Docker. `resolve` is the only place that
//! knows the layers, and it is the only place to change when one is added.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::agent::sandbox::env::secret_name;
use crate::domain::settings::{keys, store::SettingsStore};

/// The file a workspace may add. Read only when the workspace recorded its hash.
pub const PROJECT_FILE: &str = ".robi/sandbox.toml";

/// Which sandbox runs a `shell` command.
///
/// `Os` is the default and the only mechanism that needs no daemon. It is
/// Seatbelt on macOS and bubblewrap on Linux. `Container` is the gVisor
/// container, available on both platforms. Neither replaces the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mechanism {
    #[default]
    Os,
    Container,
}

impl Mechanism {
    /// Parses the stored string. Anything but `container` is [`Mechanism::Os`],
    /// so an unreadable value cannot silently escape to a wider sandbox.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("container") => Mechanism::Container,
            _ => Mechanism::Os,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mechanism::Os => "os",
            Mechanism::Container => "container",
        }
    }
}

/// One resolved setting set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub mechanism: Mechanism,
    pub image: String,
    pub runtime: String,
    pub memory_mb: u32,
    pub cpus: u32,
    pub pids_limit: u32,
    /// Endpoints the workspace declares, as `(name, port)`.
    ///
    /// Reaching one needs `service_network`: the container joins that Docker
    /// network and resolves the name. A service that only listens on the host is
    /// not reachable this way, and the escalation is an approved `network:
    /// "unrestricted"` call or `unsandboxed`.
    pub services: Vec<Service>,
    /// The Docker network holding those services.
    pub service_network: Option<String>,
    /// `sandbox_network = "off"`: every sandboxed command reaches the outside.
    ///
    /// A machine-wide trust decision the user makes once. Per call, the default
    /// is the opposite: no interface unless that call was approved for egress.
    pub network_open: bool,
    /// Project environment after the secret filter.
    pub env: BTreeMap<String, String>,
}

/// One declared dependency: what to resolve, and on which port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// The DNS name on `service_network`, and the base of the variable name.
    pub name: String,
    pub port: u16,
}

impl Service {
    /// `POSTGRES` for `postgres`. The environment names derive from it.
    pub fn upper(&self) -> String {
        self.name.to_ascii_uppercase()
    }

    /// A URL for the protocols worth guessing, and `host:port` otherwise.
    pub fn url(&self, host: &str, port: u16) -> String {
        match self.name.as_str() {
            "postgres" | "postgresql" => format!("postgres://{host}:{port}"),
            "redis" => format!("redis://{host}:{port}"),
            "mysql" | "mariadb" => format!("mysql://{host}:{port}"),
            "mongo" | "mongodb" => format!("mongodb://{host}:{port}"),
            _ => format!("{host}:{port}"),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mechanism: Mechanism::Os,
            image: "robi/agent:1".to_owned(),
            runtime: "runsc".to_owned(),
            memory_mb: 4096,
            cpus: 2,
            pids_limit: 512,
            services: Vec::new(),
            service_network: None,
            network_open: false,
            env: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn nano_cpus(&self) -> i64 {
        i64::from(self.cpus) * 1_000_000_000
    }

    pub fn memory_bytes(&self) -> i64 {
        i64::from(self.memory_mb) * 1024 * 1024
    }

    /// The interface the container starts with.
    ///
    /// `bridge` only when `sandbox_network` is `off`: the default bridge plus the
    /// daemon's NAT is what "fully networked to the outside" means.
    ///
    /// A declared service does *not* switch this to `bridge`. Doing that would put
    /// the container on the default bridge as well as its service network, so
    /// declaring a database would quietly grant full outbound egress. The service
    /// network is attached with `connect_network` instead, which works from
    /// `none`.
    pub fn network_mode(&self) -> &'static str {
        if self.network_open {
            "bridge"
        } else {
            "none"
        }
    }

    /// Whether the container already reaches the outside without a per-call
    /// approval. Then an approved `network: "unrestricted"` call needs no extra
    /// interface, because the default bridge already provides one.
    pub fn egress_open(&self) -> bool {
        self.network_open
    }
}

/// What the workspace file may say.
///
/// Every field is optional. `deny_unknown_fields` is deliberate: a typo in a
/// checked-in file should refuse the command, not quietly do nothing.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    image: Option<String>,
    runtime: Option<String>,
    memory_mb: Option<u32>,
    cpus: Option<u32>,
    pids_limit: Option<u32>,
    #[serde(default)]
    services: Vec<String>,
    service_network: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("sandbox.toml did not parse: {0}")]
    Parse(String),
    #[error("the image `{0}` is not a valid image name")]
    Image(String),
    #[error("memory_mb must be {MIN_MEMORY_MB} to {MAX_MEMORY_MB}, got {0}")]
    Memory(u32),
    #[error("cpus must be 1 to {max}, got {got}")]
    Cpus { got: u32, max: u32 },
    #[error("pids_limit must be {MIN_PIDS} to {MAX_PIDS}, got {0}")]
    Pids(u32),
    #[error("services entry `{0}` must look like `name:port`")]
    Service(String),
    #[error(
        "a declared service needs `service_network` naming the Docker network it is on. \
         A service that only listens on the host is not reachable from the sandbox: \
         publish it on all interfaces, or approve `network: \"unrestricted\"` for the call"
    )]
    ServiceNetworkRequired,
}

pub const MIN_MEMORY_MB: u32 = keys::SANDBOX_MEMORY_FLOOR_MB;
pub const MAX_MEMORY_MB: u32 = keys::SANDBOX_MEMORY_LIMIT_MB;
pub const MIN_PIDS: u32 = keys::SANDBOX_PIDS_FLOOR;
pub const MAX_PIDS: u32 = keys::SANDBOX_PIDS_MAX;

/// Resolves the mechanism from the global settings.
///
/// Separate from [`resolve`] so dispatch does not pay for a project-file read, and
/// so a build without the container sandbox can still refuse a `container` setting.
pub async fn mechanism(settings: Option<&dyn SettingsStore>) -> Mechanism {
    let Some(settings) = settings else {
        return Mechanism::Os;
    };
    Mechanism::parse(read(settings, keys::SANDBOX_MECHANISM).await.as_deref())
}

/// Resolves the three layers.
///
/// `trusted` is `sandbox_project_files` being on. Without it the workspace file
/// contributes nothing: `image` and `network` are privilege decisions, and a
/// cloned repository should not make them.
pub async fn resolve(
    settings: Option<&dyn SettingsStore>,
    workspace: &Path,
    trusted: bool,
) -> Result<Config, ConfigError> {
    let mut config = Config::default();

    if let Some(settings) = settings {
        config.mechanism =
            Mechanism::parse(read(settings, keys::SANDBOX_MECHANISM).await.as_deref());
        // `off` is the only value that opens the network; anything unreadable
        // stays sandboxed.
        config.network_open =
            read(settings, keys::SANDBOX_NETWORK).await.as_deref() == Some(keys::NETWORK_OFF);
        if let Some(value) = read(settings, keys::SANDBOX_IMAGE).await {
            config.image = value;
        }
        if let Some(value) = read(settings, keys::SANDBOX_RUNTIME).await {
            config.runtime = value;
        }
        if let Some(value) = read(settings, keys::SANDBOX_MEMORY_MB).await {
            config.memory_mb = value.parse().unwrap_or(config.memory_mb);
        }
        if let Some(value) = read(settings, keys::SANDBOX_CPUS).await {
            config.cpus = value.parse().unwrap_or(config.cpus);
        }
        if let Some(value) = read(settings, keys::SANDBOX_PIDS_LIMIT).await {
            config.pids_limit = value.parse().unwrap_or(config.pids_limit);
        }
    }

    if trusted {
        if let Some(file) = read_project_file(workspace).await? {
            apply_project_file(&mut config, file)?;
        }
    }

    validate(&config)?;
    Ok(config)
}

/// Reads `<workspace>/.robi/sandbox.toml`.
///
/// A missing file is not an error: the workspace simply inherits the global
/// settings. A present file that does not parse is an error, because reporting it
/// is the only way the user learns their edit did nothing.
async fn read_project_file(workspace: &Path) -> Result<Option<ProjectFile>, ConfigError> {
    let path = workspace.join(PROJECT_FILE);
    let owned = path.clone();
    let text = crate::agent::blocking::call(move || std::fs::read_to_string(&owned))
        .await
        .map_err(|err| ConfigError::Parse(err.to_string()))?
        .ok();
    let Some(text) = text else {
        return Ok(None);
    };
    toml::from_str(&text)
        .map(Some)
        .map_err(|err| ConfigError::Parse(err.to_string()))
}

fn apply_project_file(config: &mut Config, file: ProjectFile) -> Result<(), ConfigError> {
    if let Some(image) = file.image {
        config.image = image;
    }
    if let Some(runtime) = file.runtime {
        config.runtime = runtime;
    }
    if let Some(memory_mb) = file.memory_mb {
        config.memory_mb = memory_mb;
    }
    if let Some(cpus) = file.cpus {
        config.cpus = cpus;
    }
    if let Some(pids_limit) = file.pids_limit {
        config.pids_limit = pids_limit;
    }
    // `mechanism` is deliberately absent from `ProjectFile`. A repository does
    // not get to decide whether its own code is containerised.
    config.services = file
        .services
        .iter()
        .map(|entry| parse_service(entry))
        .collect::<Result<_, _>>()?;
    config.service_network = file.service_network;
    // A password-bearing `DATABASE_URL` must not reach a log or the transcript.
    config.env = file
        .env
        .into_iter()
        .filter(|(key, _)| !secret_name(key))
        .collect();
    Ok(())
}

/// Parses one `name:port` entry.
pub fn parse_service(entry: &str) -> Result<Service, ConfigError> {
    let (name, port) = entry
        .rsplit_once(':')
        .ok_or_else(|| ConfigError::Service(entry.to_owned()))?;
    let name_ok = !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'));
    if !name_ok {
        return Err(ConfigError::Service(entry.to_owned()));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| ConfigError::Service(entry.to_owned()))?;
    Ok(Service {
        name: name.to_owned(),
        port,
    })
}

fn validate(config: &Config) -> Result<(), ConfigError> {
    let image = &config.image;
    // The image is passed to the daemon, not to a shell, so this is about
    // catching a typo and a leading dash rather than injection.
    let image_ok =
        !image.is_empty() && !image.starts_with('-') && !image.chars().any(char::is_whitespace);
    if !image_ok {
        return Err(ConfigError::Image(image.clone()));
    }
    if !(MIN_MEMORY_MB..=MAX_MEMORY_MB).contains(&config.memory_mb) {
        return Err(ConfigError::Memory(config.memory_mb));
    }
    let max = host_cpus();
    if !(1..=max).contains(&config.cpus) {
        return Err(ConfigError::Cpus {
            got: config.cpus,
            max,
        });
    }
    if !(MIN_PIDS..=MAX_PIDS).contains(&config.pids_limit) {
        return Err(ConfigError::Pids(config.pids_limit));
    }
    if !config.services.is_empty() && config.service_network.is_none() {
        return Err(ConfigError::ServiceNetworkRequired);
    }
    Ok(())
}

fn host_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
}

async fn read(settings: &dyn SettingsStore, key: &str) -> Option<String> {
    settings
        .get(key)
        .await
        .ok()
        .flatten()
        .map(|setting| setting.value)
        .filter(|value| !value.is_empty())
}

/// The environment one command runs with.
///
/// The base environment, then one address per declared service so a test reads a
/// host instead of hardcoding one, then the project's `[env]`, which wins. The
/// secret filter already ran on `[env]`.
pub fn command_env(base: &[(String, String)], config: &Config) -> Vec<String> {
    let mut env: BTreeMap<String, String> = base
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    for service in &config.services {
        let (host, port) = (service.name.clone(), service.port);
        let upper = service.upper();
        env.entry(format!("{upper}_URL"))
            .or_insert_with(|| service.url(&host, port));
        env.insert(format!("{upper}_HOST"), host);
        env.insert(format!("{upper}_PORT"), port.to_string());
    }
    for (key, value) in &config.env {
        env.insert(key.clone(), value.clone());
    }
    env.into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::sandbox::masks::testing::scratch_dir;
    use crate::domain::settings::memory::MemorySettingsStore;
    use std::sync::Arc;

    async fn store_with(pairs: &[(&str, &str)]) -> Arc<dyn SettingsStore> {
        let store = Arc::new(MemorySettingsStore::new());
        for (key, value) in pairs {
            store.set(key, (*value).to_owned(), false).await.unwrap();
        }
        store
    }

    #[tokio::test]
    async fn defaults_are_the_os_mechanism_with_no_network() {
        let config = resolve(None, Path::new("/nonexistent"), false)
            .await
            .unwrap();
        assert_eq!(config.mechanism, Mechanism::Os);
        assert_eq!(config.image, "robi/agent:1");
        assert_eq!(config.runtime, "runsc");
        assert_eq!(config.memory_mb, 4096);
        assert_eq!(config.network_mode(), "none");
    }

    #[tokio::test]
    async fn the_global_toggle_off_opens_the_network() {
        let store = store_with(&[(keys::SANDBOX_NETWORK, keys::NETWORK_OFF)]).await;
        let config = resolve(Some(store.as_ref()), Path::new("/nonexistent"), false)
            .await
            .unwrap();
        assert!(config.network_open);
        assert_eq!(config.network_mode(), "bridge");
        assert!(config.egress_open());
    }

    #[tokio::test]
    async fn the_global_toggle_defaults_to_on() {
        // Nothing stored: the network stays sandboxed.
        let config = resolve(None, Path::new("/nonexistent"), false)
            .await
            .unwrap();
        assert!(!config.network_open);
        assert_eq!(config.network_mode(), "none");
    }

    #[tokio::test]
    async fn a_workspace_file_cannot_open_the_network() {
        // The toggle is a machine-wide trust decision. `deny_unknown_fields`
        // makes the attempt a parse error rather than a silent no-op.
        let root = scratch_dir("cfg-network");
        std::fs::write(root.join(PROJECT_FILE), "network = \"off\"\n").unwrap();
        let error = resolve(None, &root, true).await.unwrap_err();
        assert!(matches!(error, ConfigError::Parse(_)), "got {error:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn global_settings_replace_the_defaults() {
        let store = store_with(&[
            (keys::SANDBOX_MECHANISM, "container"),
            (keys::SANDBOX_MEMORY_MB, "8192"),
        ])
        .await;
        let config = resolve(Some(store.as_ref()), Path::new("/nonexistent"), false)
            .await
            .unwrap();
        assert_eq!(config.mechanism, Mechanism::Container);
        assert_eq!(config.memory_mb, 8192);
        // The toggle is untouched, so the network stays sandboxed.
        assert_eq!(config.network_mode(), "none");
    }

    #[tokio::test]
    async fn the_workspace_file_overrides_only_what_it_sets() {
        let root = scratch_dir("cfg-layer");
        std::fs::write(
            root.join(PROJECT_FILE),
            "memory_mb = 16384\nservice_network = \"myproj_default\"\nservices = [\"postgres:5432\"]\n",
        )
        .unwrap();
        let store = store_with(&[
            (keys::SANDBOX_IMAGE, "acme/base:2"),
            (keys::SANDBOX_MEMORY_MB, "2048"),
        ])
        .await;

        let config = resolve(Some(store.as_ref()), &root, true).await.unwrap();
        // The project raised memory and kept the machine's image.
        assert_eq!(config.memory_mb, 16384);
        assert_eq!(config.image, "acme/base:2");
        assert_eq!(config.services.len(), 1);
        assert_eq!(config.services[0].name, "postgres");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn an_unacknowledged_workspace_file_contributes_nothing() {
        let root = scratch_dir("cfg-untrusted");
        std::fs::write(root.join(PROJECT_FILE), "memory_mb = 16384\n").unwrap();

        let config = resolve(None, &root, false).await.unwrap();
        assert_eq!(config.memory_mb, 4096);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_project_file_cannot_choose_the_mechanism() {
        let root = scratch_dir("cfg-mechanism");
        std::fs::write(root.join(PROJECT_FILE), "mechanism = \"container\"\n").unwrap();

        let error = resolve(None, &root, true).await.unwrap_err();
        assert!(
            matches!(error, ConfigError::Parse(_)),
            "expected a parse error, got {error:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_malformed_workspace_file_is_reported() {
        let root = scratch_dir("cfg-malformed");
        std::fs::write(root.join(PROJECT_FILE), "memory_mb = ").unwrap();
        let error = resolve(None, &root, true).await.unwrap_err();
        assert!(matches!(error, ConfigError::Parse(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn out_of_range_values_are_refused() {
        let root = scratch_dir("cfg-range");
        std::fs::write(root.join(PROJECT_FILE), "memory_mb = 10\n").unwrap();
        let error = resolve(None, &root, true).await.unwrap_err();
        assert!(matches!(error, ConfigError::Memory(10)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_service_entry_needs_a_name_and_a_port() {
        assert_eq!(
            parse_service("postgres:5432"),
            Ok(Service {
                name: "postgres".to_owned(),
                port: 5432
            })
        );
        assert!(parse_service("postgres").is_err());
        assert!(parse_service(":5432").is_err());
        assert!(parse_service("postgres:http").is_err());
        assert!(parse_service("bad name:5432").is_err());
    }

    #[test]
    fn the_secret_filter_drops_a_credential_bearing_url() {
        // `[env]` filtering happens in `apply_project_file`; this pins the
        // predicate it relies on.
        assert!(secret_name("DATABASE_PASSWORD"));
        assert!(!secret_name("DATABASE_URL"));
    }

    #[test]
    fn a_declared_service_is_addressed_by_name_on_its_network() {
        let config = Config {
            services: vec![Service {
                name: "postgres".to_owned(),
                port: 5432,
            }],
            service_network: Some("myproj_default".to_owned()),
            ..Config::default()
        };

        let env = command_env(&[], &config);
        assert!(env.contains(&"POSTGRES_HOST=postgres".to_owned()));
        assert!(env.contains(&"POSTGRES_PORT=5432".to_owned()));
        assert!(env.contains(&"POSTGRES_URL=postgres://postgres:5432".to_owned()));
    }

    #[tokio::test]
    async fn a_service_without_a_network_is_refused() {
        // Without the relay there is no path to a host-only port, and a silent
        // no-op would make the test fail later with `Connection refused`. The
        // error names the escalation instead.
        let root = scratch_dir("cfg-service-no-network");
        std::fs::write(root.join(PROJECT_FILE), "services = [\"postgres:5432\"]\n").unwrap();
        let error = resolve(None, &root, true).await.unwrap_err();
        assert!(
            matches!(error, ConfigError::ServiceNetworkRequired),
            "got {error:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_env_overrides_a_generated_address() {
        let mut config = Config {
            services: vec![Service {
                name: "postgres".to_owned(),
                port: 5432,
            }],
            ..Config::default()
        };
        config.env.insert(
            "POSTGRES_URL".to_owned(),
            "postgres://app@postgres:5432/app".to_owned(),
        );
        let env = command_env(&[], &config);
        assert!(env.contains(&"POSTGRES_URL=postgres://app@postgres:5432/app".to_owned()));
    }
}
