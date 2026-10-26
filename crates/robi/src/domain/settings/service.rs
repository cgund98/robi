use std::fmt;
use std::sync::Arc;

use crate::domain::{
    error::ServiceError,
    settings::{
        keys::{self, known_setting},
        store::SettingsStore,
    },
};

/// A setting as the API should return it.
///
/// `value` is `None` when the key is on the whitelist, nothing is stored, and
/// the key has no default. A stored secret still carries its value here; the
/// HTTP layer omits it from the response.
pub struct ReadSetting {
    pub secret: bool,
    pub value: Option<String>,
}

impl fmt::Debug for ReadSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match (&self.value, self.secret) {
            (_, true) => "<redacted>",
            (Some(value), false) => value.as_str(),
            (None, false) => "<unset>",
        };
        f.debug_struct("ReadSetting")
            .field("secret", &self.secret)
            .field("value", &value)
            .finish()
    }
}

/// Validates a setting, then reads or writes the store.
pub struct SettingsService {
    pub store: Arc<dyn SettingsStore>,
}

impl SettingsService {
    /// Returns the stored value.
    ///
    /// An absent key with a default is written through the store first, so the
    /// files on disk gain that default before the value is returned. An absent
    /// key with no default returns [`ReadSetting::value`] of `None`.
    pub async fn get(&self, key: &str) -> Result<ReadSetting, ServiceError> {
        let known = require_known(key)?;
        if let Some(setting) = self.store.get(key).await? {
            return Ok(ReadSetting {
                secret: setting.secret,
                value: Some(setting.value),
            });
        }
        let Some(default_value) = known.default_value else {
            return Ok(ReadSetting {
                secret: known.secret,
                value: None,
            });
        };
        let value = default_value.to_owned();
        self.store.set(key, value.clone(), known.secret).await?;
        Ok(ReadSetting {
            secret: known.secret,
            value: Some(value),
        })
    }

    /// Reads each key in order. Every key is checked against the whitelist
    /// before any default is written.
    pub async fn get_many(&self, keys: &[String]) -> Result<Vec<ReadSetting>, ServiceError> {
        if keys.is_empty() {
            return Err(ServiceError::BadRequest(
                "at least one setting key is required".to_owned(),
            ));
        }
        for key in keys {
            require_known(key)?;
        }
        let mut settings = Vec::with_capacity(keys.len());
        for key in keys {
            settings.push(self.get(key).await?);
        }
        Ok(settings)
    }

    pub async fn set(&self, key: &str, value: String, secret: bool) -> Result<(), ServiceError> {
        let known = require_known(key)?;
        validate_value(&value)?;
        if known.secret != secret {
            return Err(ServiceError::BadRequest(if known.secret {
                format!("{key} must be stored as a secret")
            } else {
                format!("{key} must not be stored as a secret")
            }));
        }
        if matches!(
            key,
            keys::WEB_SEARCH_APPROVAL
                | keys::WEB_FETCH_APPROVAL
                | keys::PROVIDER_OPENCODE_GO
                | keys::PROVIDER_ANTHROPIC
                | keys::PROVIDER_DEEPSEEK
                | keys::SANDBOX_PROJECT_FILES
        ) && value != keys::PROVIDER_ON
            && value != keys::PROVIDER_OFF
        {
            return Err(ServiceError::BadRequest(format!("{key} must be on or off")));
        }
        // A sandbox setting chooses a mechanism and a network posture. Both are
        // two-value, and the wrong value must not be stored silently: an
        // unreadable one falls back to the narrower default at resolve time,
        // which hides the mistake.
        if key == keys::SANDBOX_MECHANISM
            && value != keys::MECHANISM_OS
            && value != keys::MECHANISM_CONTAINER
        {
            return Err(ServiceError::BadRequest(format!(
                "{key} must be {} or {}",
                keys::MECHANISM_OS,
                keys::MECHANISM_CONTAINER
            )));
        }
        if key == keys::SANDBOX_NETWORK && value != keys::NETWORK_ON && value != keys::NETWORK_OFF {
            return Err(ServiceError::BadRequest(format!(
                "{key} must be {} or {}",
                keys::NETWORK_ON,
                keys::NETWORK_OFF
            )));
        }
        if key == keys::FILE_OPEN_PATTERNS {
            validate_open_patterns(&value)?;
        }
        if key == keys::FILE_OPEN_COMMAND {
            super::open_command::validate_open_command(&value).map_err(ServiceError::BadRequest)?;
        }
        if key == keys::SANDBOX_IMAGE {
            validate_image(&value)?;
        }
        if key == keys::SANDBOX_RUNTIME && !is_container_name(&value) {
            return Err(ServiceError::BadRequest(format!(
                "{key} must be a runtime name, got {value}"
            )));
        }
        if matches!(
            key,
            keys::SANDBOX_MEMORY_MB | keys::SANDBOX_CPUS | keys::SANDBOX_PIDS_LIMIT
        ) && !in_sandbox_range(key, &value)
        {
            return Err(ServiceError::BadRequest(sandbox_range_message(key)));
        }
        if let Some(limit) = bounded_limit(key) {
            if keys::parse_bounded(Some(&value), 0, limit) == 0 {
                return Err(ServiceError::BadRequest(format!(
                    "{key} must be a whole number from 1 to {limit}"
                )));
            }
        }
        self.store.set(key, value, secret).await
    }

    /// Drop a stored key so the next read inherits. A secret key is refused.
    pub async fn remove(&self, key: &str) -> Result<(), ServiceError> {
        let known = require_known(key)?;
        if known.secret {
            return Err(ServiceError::BadRequest(format!("{key} cannot be removed")));
        }
        self.store.remove(key).await
    }
}

fn require_known(key: &str) -> Result<crate::domain::settings::keys::KnownSetting, ServiceError> {
    known_setting(key).ok_or_else(|| ServiceError::BadRequest(format!("unknown setting: {key}")))
}

/// An image reference the daemon will accept.
///
/// Not a shell word, so this rejects whitespace and a leading dash rather than
/// trying to parse a full reference grammar.
fn validate_image(value: &str) -> Result<(), ServiceError> {
    let ok =
        !value.is_empty() && !value.starts_with('-') && !value.chars().any(char::is_whitespace);
    if ok {
        Ok(())
    } else {
        Err(ServiceError::BadRequest(format!(
            "{} must look like `repo/name[:tag]`, got {value}",
            keys::SANDBOX_IMAGE
        )))
    }
}

/// A Docker runtime name: `runsc`, `runc`, and the like.
fn is_container_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
}

/// Whether a numeric sandbox setting is inside its bounds.
///
/// A value that does not parse is out of range: the resolver would otherwise fall
/// back to the default and the write would look successful.
fn in_sandbox_range(key: &str, value: &str) -> bool {
    let Ok(number) = value.parse::<u32>() else {
        return false;
    };
    match key {
        keys::SANDBOX_MEMORY_MB => {
            (keys::SANDBOX_MEMORY_FLOOR_MB..=keys::SANDBOX_MEMORY_LIMIT_MB).contains(&number)
        }
        keys::SANDBOX_PIDS_LIMIT => {
            (keys::SANDBOX_PIDS_FLOOR..=keys::SANDBOX_PIDS_MAX).contains(&number)
        }
        keys::SANDBOX_CPUS => number >= 1 && number <= host_cpus(),
        _ => true,
    }
}

fn sandbox_range_message(key: &str) -> String {
    match key {
        keys::SANDBOX_MEMORY_MB => format!(
            "{key} must be {} to {}",
            keys::SANDBOX_MEMORY_FLOOR_MB,
            keys::SANDBOX_MEMORY_LIMIT_MB
        ),
        keys::SANDBOX_PIDS_LIMIT => format!(
            "{key} must be {} to {}",
            keys::SANDBOX_PIDS_FLOOR,
            keys::SANDBOX_PIDS_MAX
        ),
        keys::SANDBOX_CPUS => format!("{key} must be 1 to {}", host_cpus()),
        _ => format!("{key} is out of range"),
    }
}

fn host_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
}

fn bounded_limit(key: &str) -> Option<u32> {
    match key {
        keys::MAX_ITERATIONS | keys::SUBAGENT_MAX_ITERATIONS => Some(keys::MAX_ITERATIONS_LIMIT),
        keys::SUBAGENT_TIMEOUT_SECONDS | keys::TOOL_TIMEOUT_SECONDS => {
            Some(keys::TIMEOUT_LIMIT_SECONDS)
        }
        _ => None,
    }
}

fn validate_value(value: &str) -> Result<(), ServiceError> {
    if value.is_empty() {
        return Err(ServiceError::BadRequest("value must not be empty".into()));
    }
    Ok(())
}

/// Each non-blank line is a glob matched against the whole workspace-relative path.
///
/// `*` is any run of characters, including `/`. `?` is one character. Everything
/// else is literal.
fn validate_open_patterns(value: &str) -> Result<(), ServiceError> {
    for line in value.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Err(err) = regex::Regex::new(&glob_to_regex(line)) {
            return Err(ServiceError::BadRequest(format!(
                "file_open_patterns has an invalid pattern: {err}"
            )));
        }
    }
    Ok(())
}

fn glob_to_regex(pattern: &str) -> String {
    let mut out = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            other => out.push_str(&regex::escape(&other.to_string())),
        }
    }
    out.push('$');
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::settings::{
        keys::{self, BASE_URL, MODEL, OPENCODE_GO_API_KEY},
        memory::MemorySettingsStore,
    };

    fn service() -> SettingsService {
        SettingsService {
            store: Arc::new(MemorySettingsStore::new()),
        }
    }

    #[tokio::test]
    async fn a_key_outside_the_whitelist_is_rejected() {
        let service = service();
        let error = service
            .set("custom_token", "abc".into(), true)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("unknown setting: custom_token".into())
        );
        let read = service.get("custom_token").await.unwrap_err();
        assert_eq!(
            read,
            ServiceError::BadRequest("unknown setting: custom_token".into())
        );
    }

    #[tokio::test]
    async fn get_many_rejects_an_unknown_key_before_writing_defaults() {
        let store = Arc::new(MemorySettingsStore::new());
        let service = SettingsService {
            store: store.clone(),
        };
        let error = service
            .get_many(&[MODEL.to_owned(), "custom_token".to_owned()])
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("unknown setting: custom_token".into())
        );
        assert!(store.get(MODEL).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_many_returns_keys_in_request_order() {
        let service = service();
        let settings = service
            .get_many(&[BASE_URL.to_owned(), OPENCODE_GO_API_KEY.to_owned()])
            .await
            .unwrap();
        assert_eq!(settings.len(), 2);
        assert!(!settings[0].secret);
        assert!(settings[0].value.is_none());
        assert!(settings[1].secret);
        assert!(settings[1].value.is_none());
    }

    #[tokio::test]
    async fn an_empty_value_is_rejected() {
        let error = service()
            .set(MODEL, String::new(), false)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("value must not be empty".into())
        );
    }

    #[tokio::test]
    async fn a_known_key_must_use_its_secret_flag() {
        let service = service();
        let as_plain = service
            .set(OPENCODE_GO_API_KEY, "sk-live".into(), false)
            .await
            .unwrap_err();
        assert_eq!(
            as_plain,
            ServiceError::BadRequest("opencode_go_api_key must be stored as a secret".into())
        );

        let as_secret = service
            .set(MODEL, "glm-5.3".into(), true)
            .await
            .unwrap_err();
        assert_eq!(
            as_secret,
            ServiceError::BadRequest("model must not be stored as a secret".into())
        );

        let unset_key = service.get(OPENCODE_GO_API_KEY).await.unwrap();
        assert!(unset_key.value.is_none());
        assert!(unset_key.secret);
    }

    #[tokio::test]
    async fn an_unset_model_is_flushed_as_the_default() {
        let store = Arc::new(MemorySettingsStore::new());
        let service = SettingsService {
            store: Arc::clone(&store) as Arc<dyn SettingsStore>,
        };

        let setting = service.get(MODEL).await.unwrap();
        assert_eq!(setting.value.as_deref(), Some(keys::DEFAULT_MODEL));
        assert!(!setting.secret);

        let stored = store.get(MODEL).await.unwrap().unwrap();
        assert_eq!(stored.value, keys::DEFAULT_MODEL);
        assert!(!stored.secret);
    }

    #[tokio::test]
    async fn an_unset_key_without_a_default_returns_none() {
        let store = Arc::new(MemorySettingsStore::new());
        let service = SettingsService {
            store: Arc::clone(&store) as Arc<dyn SettingsStore>,
        };

        let base_url = service.get(BASE_URL).await.unwrap();
        assert_eq!(base_url.value, None);
        assert!(!base_url.secret);
        assert!(store.get(BASE_URL).await.unwrap().is_none());

        let api_key = service.get(OPENCODE_GO_API_KEY).await.unwrap();
        assert_eq!(api_key.value, None);
        assert!(api_key.secret);
        assert!(store.get(OPENCODE_GO_API_KEY).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_removed_lsp_key_is_no_longer_known() {
        // S4 removed the setting along with the tools it gated. A client that
        // still writes it gets the same 400 an unknown key always gets.
        let service = SettingsService {
            store: Arc::new(MemorySettingsStore::new()) as Arc<dyn SettingsStore>,
        };
        let error = service.set("lsp", "on".into(), false).await.unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("unknown setting: lsp".into())
        );
    }

    #[tokio::test]
    async fn web_approvals_default_to_on() {
        let store = Arc::new(MemorySettingsStore::new());
        let service = SettingsService {
            store: Arc::clone(&store) as Arc<dyn SettingsStore>,
        };

        for key in [keys::WEB_SEARCH_APPROVAL, keys::WEB_FETCH_APPROVAL] {
            let setting = service.get(key).await.unwrap();
            assert_eq!(setting.value.as_deref(), Some(keys::APPROVAL_ON));
            assert!(keys::approval_required(setting.value.as_deref()));
        }
    }

    #[tokio::test]
    async fn iteration_caps_default_and_reject_out_of_range() {
        let service = service();
        let primary = service.get(keys::MAX_ITERATIONS).await.unwrap();
        assert_eq!(primary.value.as_deref(), Some(keys::DEFAULT_MAX_ITERATIONS));
        let child = service.get(keys::SUBAGENT_MAX_ITERATIONS).await.unwrap();
        assert_eq!(
            child.value.as_deref(),
            Some(keys::DEFAULT_SUBAGENT_MAX_ITERATIONS)
        );

        let error = service
            .set(keys::MAX_ITERATIONS, "0".into(), false)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("max_iterations must be a whole number from 1 to 500".into())
        );
    }

    #[tokio::test]
    async fn file_open_patterns_accept_a_glob() {
        let service = service();
        service
            .set(keys::FILE_OPEN_PATTERNS, "*.rs\n*.png".into(), false)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn file_open_command_requires_a_path_placeholder() {
        let service = service();
        service
            .set(
                keys::FILE_OPEN_COMMAND,
                "zed {file_path}:{line}".into(),
                false,
            )
            .await
            .unwrap();
        let error = service
            .set(keys::FILE_OPEN_COMMAND, "zed".into(), false)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("file_open_command must contain {file_path}".into())
        );
    }
}
