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
        if key == keys::LSP && value != keys::LSP_ON && value != keys::LSP_OFF {
            return Err(ServiceError::BadRequest("lsp must be on or off".into()));
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

fn validate_value(value: &str) -> Result<(), ServiceError> {
    if value.is_empty() {
        return Err(ServiceError::BadRequest("value must not be empty".into()));
    }
    Ok(())
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
    async fn lsp_defaults_to_on_and_rejects_other_values() {
        let store = Arc::new(MemorySettingsStore::new());
        let service = SettingsService {
            store: Arc::clone(&store) as Arc<dyn SettingsStore>,
        };

        let setting = service.get(keys::LSP).await.unwrap();
        assert_eq!(setting.value.as_deref(), Some(keys::LSP_ON));
        assert!(keys::lsp_enabled(setting.value.as_deref()));

        service
            .set(keys::LSP, keys::LSP_OFF.into(), false)
            .await
            .unwrap();
        let off = service.get(keys::LSP).await.unwrap();
        assert!(!keys::lsp_enabled(off.value.as_deref()));

        let error = service
            .set(keys::LSP, "true".into(), false)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("lsp must be on or off".into())
        );
    }
}
