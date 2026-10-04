//! Names the factory and the settings service share.
//!
//! The HTTP API only reads and writes these keys. Each one has a fixed secret
//! flag. A default, when there is one, is written through the store on the
//! first read so the files and later reads agree.

/// OpenCode Go bearer credential. Always a secret. No default.
pub const OPENCODE_GO_API_KEY: &str = "opencode_go_api_key";

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
pub const BASE_URL: &str = "base_url";

/// Extra system-prompt text. Not a secret. Unset adds no user block.
pub const SYSTEM_PROMPT: &str = "system_prompt";

/// `on` or `off`. Not a secret. Absent and the stored default are `on`.
pub const LSP: &str = "lsp";

/// Language-server tools are registered.
pub const LSP_ON: &str = "on";

/// Language-server tools are left out of the registry.
pub const LSP_OFF: &str = "off";

/// True unless the stored value is [`LSP_OFF`].
pub fn lsp_enabled(value: Option<&str>) -> bool {
    value != Some(LSP_OFF)
}

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
pub const DEFAULT_MODEL: &str = "glm-5.3";

/// One key the API is allowed to read and write.
pub struct KnownSetting {
    pub secret: bool,
    /// Written to the store on the first read when the key is absent.
    pub default_value: Option<&'static str>,
}

/// The whitelist entry for `key`, or `None` when the API must refuse it.
pub fn known_setting(key: &str) -> Option<KnownSetting> {
    match key {
        OPENCODE_GO_API_KEY | BRAVE_SEARCH_API_KEY => Some(KnownSetting {
            secret: true,
            default_value: None,
        }),
        MODEL => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_MODEL),
        }),
        LSP => Some(KnownSetting {
            secret: false,
            default_value: Some(LSP_ON),
        }),
        WEB_SEARCH_APPROVAL | WEB_FETCH_APPROVAL => Some(KnownSetting {
            secret: false,
            default_value: Some(APPROVAL_ON),
        }),
        REASONING_EFFORT
        | BASE_URL
        | SYSTEM_PROMPT
        | MODEL_ASK
        | MODEL_PLAN
        | MODEL_AGENT
        | REASONING_EFFORT_ASK
        | REASONING_EFFORT_PLAN
        | REASONING_EFFORT_AGENT
        | PATH_ALLOW_READ
        | PATH_ALLOW_WRITE
        | PATH_ENTRIES => Some(KnownSetting {
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
        _ => None,
    }
}
