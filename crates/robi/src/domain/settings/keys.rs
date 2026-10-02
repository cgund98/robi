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

/// Optional provider base URL. Not a secret. Unset leaves the provider default.
pub const BASE_URL: &str = "base_url";

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
        OPENCODE_GO_API_KEY => Some(KnownSetting {
            secret: true,
            default_value: None,
        }),
        MODEL => Some(KnownSetting {
            secret: false,
            default_value: Some(DEFAULT_MODEL),
        }),
        REASONING_EFFORT | BASE_URL => Some(KnownSetting {
            secret: false,
            default_value: None,
        }),
        _ => None,
    }
}
