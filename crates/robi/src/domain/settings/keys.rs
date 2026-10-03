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
        REASONING_EFFORT
        | BASE_URL
        | SYSTEM_PROMPT
        | MODEL_ASK
        | MODEL_PLAN
        | MODEL_AGENT
        | REASONING_EFFORT_ASK
        | REASONING_EFFORT_PLAN
        | REASONING_EFFORT_AGENT => Some(KnownSetting {
            secret: false,
            default_value: None,
        }),
        _ => None,
    }
}
