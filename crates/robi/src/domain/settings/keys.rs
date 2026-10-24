//! Names the factory and the settings service share.
//!
//! The HTTP API only reads and writes these keys. Each one has a fixed secret
//! flag. A default, when there is one, is written through the store on the
//! first read so the files and later reads agree.

/// OpenCode Go bearer credential. Always a secret. No default.
pub const OPENCODE_GO_API_KEY: &str = "opencode_go_api_key";

/// Anthropic credential (`x-api-key`). Always a secret. No default.
pub const ANTHROPIC_API_KEY: &str = "anthropic_api_key";

/// DeepSeek bearer credential. Always a secret. No default.
pub const DEEPSEEK_API_KEY: &str = "deepseek_api_key";

/// Model id passed to the provider. Not a secret.
pub const MODEL: &str = "model";

/// `low`, `medium`, or `high`. Not a secret. Unset leaves the provider default.
pub const REASONING_EFFORT: &str = "reasoning_effort";

/// Optional model for ask mode. Empty inherits [`MODEL`].
pub const MODEL_ASK: &str = "model_ask";

/// Optional model for plan mode. Empty inherits [`MODEL`].
pub const MODEL_PLAN: &str = "model_plan";

/// Optional model for agent mode. Empty inherits [`MODEL`].
pub const MODEL_AGENT: &str = "model_agent";

/// Optional effort for ask mode. Empty inherits [`REASONING_EFFORT`].
pub const REASONING_EFFORT_ASK: &str = "reasoning_effort_ask";

/// Optional effort for plan mode. Empty inherits [`REASONING_EFFORT`].
pub const REASONING_EFFORT_PLAN: &str = "reasoning_effort_plan";

/// Optional effort for agent mode. Empty inherits [`REASONING_EFFORT`].
pub const REASONING_EFFORT_AGENT: &str = "reasoning_effort_agent";

/// Settings key for one mode's model override.
pub fn model_key(mode: crate::domain::chat_session::model::AgentMode) -> &'static str {
    use crate::domain::chat_session::model::AgentMode;
    match mode {
        AgentMode::Ask => MODEL_ASK,
        AgentMode::Plan => MODEL_PLAN,
        AgentMode::Agent => MODEL_AGENT,
    }
}

/// Settings key for one mode's effort override.
pub fn effort_key(mode: crate::domain::chat_session::model::AgentMode) -> &'static str {
    use crate::domain::chat_session::model::AgentMode;
    match mode {
        AgentMode::Ask => REASONING_EFFORT_ASK,
        AgentMode::Plan => REASONING_EFFORT_PLAN,
        AgentMode::Agent => REASONING_EFFORT_AGENT,
    }
}

/// Optional provider base URL. Not a secret. Unset leaves the provider default.
/// Applies to OpenCode Go and Anthropic. DeepSeek uses [`DEEPSEEK_BASE_URL`].
pub const BASE_URL: &str = "base_url";

/// Optional DeepSeek base URL. Not a secret. Unset leaves `https://api.deepseek.com`.
pub const DEEPSEEK_BASE_URL: &str = "deepseek_base_url";

/// `on` or `off`. Not a secret. Absent and the stored default are `on`.
pub const PROVIDER_OPENCODE_GO: &str = "provider_opencode_go";

/// `on` or `off`. Not a secret. Absent and the stored default are `on`.
pub const PROVIDER_ANTHROPIC: &str = "provider_anthropic";

/// `on` or `off`. Not a secret. Absent and the stored default are `on`.
pub const PROVIDER_DEEPSEEK: &str = "provider_deepseek";

/// The provider is offered in the model list and can drive a turn.
pub const PROVIDER_ON: &str = "on";

/// The provider is left out of the model list, and a turn that needs it fails.
pub const PROVIDER_OFF: &str = "off";

/// True unless the stored value is [`PROVIDER_OFF`].
pub fn provider_enabled(value: Option<&str>) -> bool {
    value != Some(PROVIDER_OFF)
}

/// Extra system-prompt text. Not a secret. Unset adds no user block.
pub const SYSTEM_PROMPT: &str = "system_prompt";

/// Brave Search subscription token. Always a secret. No default.
pub const BRAVE_SEARCH_API_KEY: &str = "brave_search_api_key";

/// `on` or `off`. Not a secret. `on` makes every `web_search` wait for approval.
pub const WEB_SEARCH_APPROVAL: &str = "web_search_approval";

/// `on` or `off`. Not a secret. `on` makes the first `web_fetch` to a host wait.
pub const WEB_FETCH_APPROVAL: &str = "web_fetch_approval";

/// Newline-separated paths appended to the session read allow list.
/// A line may start with `~/`. Not a secret. Absent adds nothing.
pub const PATH_ALLOW_READ: &str = "path_allow_read";

/// Newline-separated paths appended to the session write allow list.
/// A line may start with `~/`. Not a secret. Absent adds nothing.
pub const PATH_ALLOW_WRITE: &str = "path_allow_write";

/// Newline-separated directories appended to the sandbox `PATH`.
/// Each one is also a read allow. A line may start with `~/`. Not a secret.
pub const PATH_ENTRIES: &str = "path_entries";

/// Newline-separated globs. `*` matches any run of characters, including `/`.
/// A workspace-relative path that matches one is opened in the system app.
/// Anything else opens in the file viewer. Not a secret. Absent matches nothing.
pub const FILE_OPEN_PATTERNS: &str = "file_open_patterns";

/// Command that opens a path matched by [`FILE_OPEN_PATTERNS`].
///
/// `{file_path}` is the absolute path and `{line}` is a 1-based line.
/// Not a secret. Absent uses the system opener.
pub const FILE_OPEN_COMMAND: &str = "file_open_command";

/// Model turns allowed in one primary-agent run. Not a secret.
pub const MAX_ITERATIONS: &str = "max_iterations";

/// Model turns allowed in one subagent run. Both child modes use it. Not a secret.
pub const SUBAGENT_MAX_ITERATIONS: &str = "subagent_max_iterations";

/// Written on the first read of [`MAX_ITERATIONS`].
pub const DEFAULT_MAX_ITERATIONS: &str = "50";

/// Written on the first read of [`SUBAGENT_MAX_ITERATIONS`].
pub const DEFAULT_SUBAGENT_MAX_ITERATIONS: &str = "50";

/// Inclusive upper bound for either iteration setting.
pub const MAX_ITERATIONS_LIMIT: u32 = 500;

/// Wall-clock seconds for one subagent. Not a secret.
pub const SUBAGENT_TIMEOUT_SECONDS: &str = "subagent_timeout_seconds";

/// Wall-clock seconds for one `shell` command. Not a secret.
pub const TOOL_TIMEOUT_SECONDS: &str = "tool_timeout_seconds";

/// `os` or `container`. Not a secret.
///
/// `os` is Seatbelt on macOS and bubblewrap on Linux, and it is the default.
/// `container` runs the command in a gVisor container over the Docker API, on
/// both platforms. Neither replaces the other.
pub const SANDBOX_MECHANISM: &str = "sandbox_mechanism";

/// Docker image for the container sandbox. Not a secret.
pub const SANDBOX_IMAGE: &str = "sandbox_image";

/// Container runtime name, `runsc` by default. Not a secret.
pub const SANDBOX_RUNTIME: &str = "sandbox_runtime";

/// Container memory limit in MiB. Not a secret.
pub const SANDBOX_MEMORY_MB: &str = "sandbox_memory_mb";

/// Container CPU limit, in whole CPUs. Not a secret.
pub const SANDBOX_CPUS: &str = "sandbox_cpus";

/// Container process limit. Not a secret.
pub const SANDBOX_PIDS_LIMIT: &str = "sandbox_pids_limit";

/// `deny` or `unrestricted`. Not a secret.
///
/// `deny` is the default: one command may be approved for egress, but the
/// workspace does not get it by default. A declared service is a third state and
/// lives in the workspace file, not here.
pub const SANDBOX_NETWORK: &str = "sandbox_network";

/// `on` or `off`. Not a secret. `on` lets a workspace's `.robi/sandbox.toml`
/// apply, under the settings screen's acknowledgement.
pub const SANDBOX_PROJECT_FILES: &str = "sandbox_project_files";

/// The stored value that lets a workspace file apply.
pub const PROJECT_FILES_ON: &str = "on";

/// Written on the first read of [`SANDBOX_PROJECT_FILES`]. Off by default: a
/// cloned repository should not name the image it runs in.
pub const DEFAULT_SANDBOX_PROJECT_FILES: &str = "off";

/// Written on the first read of [`SANDBOX_MECHANISM`].
pub const DEFAULT_SANDBOX_MECHANISM: &str = "os";

/// Written on the first read of [`SANDBOX_IMAGE`].
pub const DEFAULT_SANDBOX_IMAGE: &str = "robi/agent:0.1.0";

/// Written on the first read of [`SANDBOX_RUNTIME`].
pub const DEFAULT_SANDBOX_RUNTIME: &str = "runsc";

/// Written on the first read of [`SANDBOX_MEMORY_MB`].
pub const DEFAULT_SANDBOX_MEMORY_MB: &str = "4096";

/// Written on the first read of [`SANDBOX_CPUS`].
pub const DEFAULT_SANDBOX_CPUS: &str = "2";

/// Written on the first read of [`SANDBOX_PIDS_LIMIT`].
pub const DEFAULT_SANDBOX_PIDS_LIMIT: &str = "512";

/// Written on the first read of [`SANDBOX_NETWORK`].
pub const DEFAULT_SANDBOX_NETWORK: &str = "deny";

/// Every command in the workspace has egress. Set by `sandbox_network`.
pub const NETWORK_UNRESTRICTED: &str = "unrestricted";

/// The sandbox runs in a gVisor container. Set by `sandbox_mechanism`.
pub const MECHANISM_CONTAINER: &str = "container";

/// The sandbox runs under Seatbelt or bubblewrap. Set by `sandbox_mechanism`.
pub const MECHANISM_OS: &str = "os";

/// Inclusive upper bound for a container memory limit, in MiB. 64 GiB.
pub const SANDBOX_MEMORY_LIMIT_MB: u32 = 65_536;

/// Inclusive lower bound for a container memory limit, in MiB.
pub const SANDBOX_MEMORY_FLOOR_MB: u32 = 256;

/// Inclusive upper bound for a container process limit.
pub const SANDBOX_PIDS_MAX: u32 = 8_192;

/// Inclusive lower bound for a container process limit.
pub const SANDBOX_PIDS_FLOOR: u32 = 64;

/// Written on the first read of either timeout key. Two minutes.
pub const DEFAULT_TIMEOUT_SECONDS: &str = "120";

/// Inclusive upper bound for either timeout, in seconds. One hour.
pub const TIMEOUT_LIMIT_SECONDS: u32 = 3600;

/// Parse a stored iteration cap. An absent or unusable value is `default`.
pub fn parse_iterations(value: Option<&str>, default: u32) -> u32 {
    parse_bounded(value, default, MAX_ITERATIONS_LIMIT)
}

/// Parse a whole number from 1 through `max`. An absent or unusable value is `default`.
pub fn parse_bounded(value: Option<&str>, default: u32, max: u32) -> u32 {
    let Some(value) = value else {
        return default;
    };
    match value.parse::<u32>() {
        Ok(n) if (1..=max).contains(&n) => n,
        _ => default,
    }
}

/// The tool waits for an explicit approval.
pub const APPROVAL_ON: &str = "on";

/// The tool runs without an approval card.
pub const APPROVAL_OFF: &str = "off";

/// True unless the stored value is [`APPROVAL_OFF`].
pub fn approval_required(value: Option<&str>) -> bool {
    value != Some(APPROVAL_OFF)
}

/// Used when [`MODEL`] has not been stored yet.
pub const DEFAULT_MODEL: &str = "ocg_glm-5.3";

/// One key the API is allowed to read and write.
pub struct KnownSetting {
    pub secret: bool,
    /// Written to the store on the first read when the key is absent.
    pub default_value: Option<&'static str>,
}

/// The whitelist entry for `key`, or `None` when the API must refuse it.
pub fn known_setting(key: &str) -> Option<KnownSetting> {
    match key {
        OPENCODE_GO_API_KEY | ANTHROPIC_API_KEY | DEEPSEEK_API_KEY | BRAVE_SEARCH_API_KEY => {
            Some(KnownSetting {
                secret: true,
                default_value: None,
            })
        }
        MODEL => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_MODEL),
        }),
        PROVIDER_OPENCODE_GO | PROVIDER_ANTHROPIC | PROVIDER_DEEPSEEK => Some(KnownSetting {
            secret: false,
            default_value: Some(PROVIDER_ON),
        }),
        WEB_SEARCH_APPROVAL | WEB_FETCH_APPROVAL => Some(KnownSetting {
            secret: false,
            default_value: Some(APPROVAL_ON),
        }),
        REASONING_EFFORT
        | BASE_URL
        | DEEPSEEK_BASE_URL
        | SYSTEM_PROMPT
        | MODEL_ASK
        | MODEL_PLAN
        | MODEL_AGENT
        | REASONING_EFFORT_ASK
        | REASONING_EFFORT_PLAN
        | REASONING_EFFORT_AGENT
        | PATH_ALLOW_READ
        | PATH_ALLOW_WRITE
        | PATH_ENTRIES
        | FILE_OPEN_PATTERNS
        | FILE_OPEN_COMMAND => Some(KnownSetting {
            secret: false,
            default_value: None,
        }),
        MAX_ITERATIONS => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_MAX_ITERATIONS),
        }),
        SUBAGENT_MAX_ITERATIONS => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SUBAGENT_MAX_ITERATIONS),
        }),
        SUBAGENT_TIMEOUT_SECONDS | TOOL_TIMEOUT_SECONDS => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_TIMEOUT_SECONDS),
        }),
        SANDBOX_MECHANISM => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SANDBOX_MECHANISM),
        }),
        SANDBOX_IMAGE => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SANDBOX_IMAGE),
        }),
        SANDBOX_RUNTIME => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SANDBOX_RUNTIME),
        }),
        SANDBOX_MEMORY_MB => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SANDBOX_MEMORY_MB),
        }),
        SANDBOX_CPUS => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SANDBOX_CPUS),
        }),
        SANDBOX_PIDS_LIMIT => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SANDBOX_PIDS_LIMIT),
        }),
        SANDBOX_NETWORK => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SANDBOX_NETWORK),
        }),
        SANDBOX_PROJECT_FILES => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_SANDBOX_PROJECT_FILES),
        }),
        _ => None,
    }
}
