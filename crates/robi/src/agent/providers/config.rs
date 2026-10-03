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
/// OpenCode Go's configuration writes `opencode-go/<model-id>`; what a request
/// carries is `<model-id>` alone, which is what this holds.
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
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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
        }
    }

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
        let settings =
            ProviderSettings::opencode_go(ApiKey::new("sk-secret-value"), ModelId::new("glm-5.3"));
        let rendered = format!("{settings:?}");
        assert!(
            !rendered.contains("sk-secret-value"),
            "settings debug output must not contain the key: {rendered}"
        );
    }

    #[test]
    fn the_url_joins_without_a_double_slash() {
        let mut settings = ProviderSettings::opencode_go(ApiKey::new("k"), ModelId::new("glm-5.3"));
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
}
