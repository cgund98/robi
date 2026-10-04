//! What an adapter needs to build a request.
//!
//! `ProviderSettings` is plain data. Nothing here reads an environment variable,
//! a file, or a keychain: the credential arrives already loaded, and the layer
//! that loads it (settings plus the OS keychain) belongs to M2. The provider
//! factory is the seam that layer plugs into.

use std::fmt;
use std::time::Duration;

use http::HeaderName;

use super::retry::RetryPolicy;

/// Identifies a provider, as configuration names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A model id, as the provider knows it.
///
/// Configuration and the catalog store a **prefixed** id: `ocg_` for OpenCode Go,
/// `ant_` for Anthropic. The prefix names the provider, so one flat namespace
/// resolves the provider without a separate setting or a catalog union (A11). What
/// a request carries is the bare id, which is what [`ModelId::wire_id`] returns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ModelId(String);

impl ModelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The id with its provider prefix removed, for the wire.
    pub fn wire_id(&self) -> &str {
        strip_model_prefix(&self.0)
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The provider prefix OpenCode Go model ids carry.
pub const OPENCODE_GO_PREFIX: &str = "ocg_";

/// The provider prefix Anthropic model ids carry.
pub const ANTHROPIC_PREFIX: &str = "ant_";

/// Which provider a model id belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenCodeGo,
    Anthropic,
}

impl ProviderKind {
    /// The provider a prefixed model id names.
    ///
    /// An id with neither known prefix is a **legacy** OpenCode Go id: settings and
    /// session rows written before the prefix existed hold a bare id, and reading
    /// them as OpenCode Go keeps every stored session working (A11).
    pub fn of(model: &str) -> Self {
        if model.starts_with(ANTHROPIC_PREFIX) {
            ProviderKind::Anthropic
        } else {
            ProviderKind::OpenCodeGo
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenCodeGo => "opencode-go",
            ProviderKind::Anthropic => "anthropic",
        }
    }
}

/// Drop a known provider prefix, leaving the bare id the wire wants.
pub fn strip_model_prefix(model: &str) -> &str {
    model
        .strip_prefix(OPENCODE_GO_PREFIX)
        .or_else(|| model.strip_prefix(ANTHROPIC_PREFIX))
        .unwrap_or(model)
}

/// Prefix a bare id for `kind` if it does not already carry one.
///
/// A legacy bare id is a no-op for OpenCode Go only when it is already bare; the
/// caller decides whether to migrate. An id already carrying its prefix is
/// returned unchanged.
pub fn prefixed_model_id(model: &str, kind: ProviderKind) -> String {
    let prefix = match kind {
        ProviderKind::OpenCodeGo => OPENCODE_GO_PREFIX,
        ProviderKind::Anthropic => ANTHROPIC_PREFIX,
    };
    if model.starts_with(OPENCODE_GO_PREFIX) || model.starts_with(ANTHROPIC_PREFIX) {
        model.to_owned()
    } else {
        format!("{prefix}{model}")
    }
}

/// An API key that does not print itself.
///
/// `Debug` and `Display` both redact, so a key cannot reach a log line, an error
/// message, or a debug-formatted request through the ordinary `{:?}` and `{}`
/// paths. Reading the value needs [`ApiKey::expose`], which is named so that a
/// call site is visible in review.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The key itself. The name is deliberate: every call is a decision.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// How much reasoning a model should spend, when it supports the setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }
}

/// Everything one configured model needs.
#[derive(Debug, Clone)]
pub struct ProviderSettings {
    pub id: ProviderId,
    /// Ends before the path, e.g. `https://opencode.ai/zen/go/v1`.
    pub base_url: String,
    pub api_key: ApiKey,
    pub model: ModelId,
    /// Prepended as a `system` message. The transcript has no `system` role, so
    /// the adapter injects this at request-build time.
    pub system_prompt: String,
    /// The header that carries the conversation key, when the provider wants one.
    pub session_header: Option<HeaderName>,
    pub user_agent: String,
    /// Bounds the wait for response headers only. A stream may run longer.
    pub header_timeout: Duration,
    /// Bounds the gap between two streamed chunks.
    pub chunk_timeout: Duration,
    pub retry: RetryPolicy,
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Overrides the session key. `None` uses the loop's `SessionId`.
    pub session_key_override: Option<String>,
    /// The output-token cap the provider requires. `None` for a provider that does
    /// not need one (chat-completions).
    pub max_tokens: Option<u64>,
}

impl ProviderSettings {
    /// OpenCode Go's endpoint, header name, and timeouts.
    pub fn opencode_go(api_key: ApiKey, model: ModelId) -> Self {
        Self {
            id: ProviderId::new("opencode-go"),
            base_url: "https://opencode.ai/zen/go/v1".to_owned(),
            api_key,
            model,
            system_prompt: String::new(),
            session_header: Some(HeaderName::from_static("x-opencode-session")),
            // The vendor asks a client to name itself rather than send a library
            // default, because it monitors traffic for abuse.
            user_agent: concat!("robi/", env!("CARGO_PKG_VERSION")).to_owned(),
            header_timeout: Duration::from_secs(300),
            chunk_timeout: Duration::from_secs(300),
            retry: RetryPolicy::default(),
            reasoning_effort: None,
            session_key_override: None,
            max_tokens: None,
        }
    }

    /// Anthropic's Messages endpoint.
    ///
    /// `x-api-key` and `anthropic-version` replace the bearer/session pair OpenCode
    /// Go uses; there is no session header (A6). `max_tokens` is required by the
    /// Messages API and is set from the catalog before the model is built (A5).
    pub fn anthropic(api_key: ApiKey, model: ModelId) -> Self {
        Self {
            id: ProviderId::new("anthropic"),
            base_url: "https://api.anthropic.com/v1".to_owned(),
            api_key,
            model,
            system_prompt: String::new(),
            session_header: None,
            user_agent: concat!("robi/", env!("CARGO_PKG_VERSION")).to_owned(),
            header_timeout: Duration::from_secs(300),
            chunk_timeout: Duration::from_secs(300),
            retry: RetryPolicy::default(),
            reasoning_effort: None,
            session_key_override: None,
            max_tokens: None,
        }
    }

    /// The Anthropic version header every request must carry.
    pub const ANTHROPIC_VERSION: &'static str = "2023-06-01";

    /// The beta header Opus 4.5 needs before it accepts `output_config.effort`.
    pub const ANTHROPIC_EFFORT_BETA: &'static str = "effort-2025-11-24";

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn with_reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.reasoning_effort = Some(effort);
        self
    }

    pub fn with_header_timeout(mut self, timeout: Duration) -> Self {
        self.header_timeout = timeout;
        self
    }

    pub fn with_chunk_timeout(mut self, timeout: Duration) -> Self {
        self.chunk_timeout = timeout;
        self
    }

    /// The chat-completions URL this configuration posts to.
    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// The Anthropic Messages URL this configuration posts to.
    pub fn messages_url(&self) -> String {
        format!("{}/messages", self.base_url.trim_end_matches('/'))
    }

    /// The URL this configuration will post to, by provider.
    pub fn messages_or_chat_url(&self) -> String {
        match ProviderKind::of(self.model.as_str()) {
            ProviderKind::Anthropic => self.messages_url(),
            ProviderKind::OpenCodeGo => self.chat_completions_url(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_does_not_print_itself() {
        let key = ApiKey::new("sk-secret-value");
        assert_eq!(format!("{key}"), "<redacted>");
        assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
        assert_eq!(key.expose(), "sk-secret-value");
    }

    #[test]
    fn settings_debug_does_not_leak_the_key() {
        // `ProviderSettings` derives `Debug`, so this holds only because `ApiKey`
        // redacts. Assert it, because a change to either would leak a credential
        // into any log that prints the settings.
        let settings = ProviderSettings::opencode_go(
            ApiKey::new("sk-secret-value"),
            ModelId::new("ocg_glm-5.3"),
        );
        let rendered = format!("{settings:?}");
        assert!(
            !rendered.contains("sk-secret-value"),
            "settings debug output must not contain the key: {rendered}"
        );
    }

    #[test]
    fn the_url_joins_without_a_double_slash() {
        let mut settings =
            ProviderSettings::opencode_go(ApiKey::new("k"), ModelId::new("ocg_glm-5.3"));
        assert_eq!(
            settings.chat_completions_url(),
            "https://opencode.ai/zen/go/v1/chat/completions"
        );
        settings.base_url = "https://example.test/v1/".to_owned();
        assert_eq!(
            settings.chat_completions_url(),
            "https://example.test/v1/chat/completions"
        );
    }

    #[test]
    fn the_messages_url_joins_without_a_double_slash() {
        let settings =
            ProviderSettings::anthropic(ApiKey::new("k"), ModelId::new("ant_claude-sonnet-4-6"));
        assert_eq!(
            settings.messages_url(),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn a_model_id_strips_only_a_known_prefix() {
        assert_eq!(ModelId::new("ocg_glm-5.3").wire_id(), "glm-5.3");
        assert_eq!(
            ModelId::new("ant_claude-sonnet-4-6").wire_id(),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            ModelId::new("glm-5.3").wire_id(),
            "glm-5.3",
            "a legacy bare id is unchanged"
        );
    }

    #[test]
    fn the_prefix_selects_the_provider_and_bare_ids_are_legacy_opencode_go() {
        assert_eq!(ProviderKind::of("ocg_glm-5.3"), ProviderKind::OpenCodeGo);
        assert_eq!(
            ProviderKind::of("ant_claude-opus-4-6"),
            ProviderKind::Anthropic
        );
        assert_eq!(
            ProviderKind::of("glm-5.3"),
            ProviderKind::OpenCodeGo,
            "an id with neither prefix is a legacy OpenCode Go id"
        );
    }
}
