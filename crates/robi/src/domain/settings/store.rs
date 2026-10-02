use std::fmt;

use async_trait::async_trait;

use crate::domain::error::ServiceError;

/// One stored string, and whether it is a secret.
///
/// `Debug` redacts secret values so a log of the map cannot print a credential.
#[derive(Clone, PartialEq, Eq)]
pub struct Setting {
    pub value: String,
    pub secret: bool,
}

impl fmt::Debug for Setting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = if self.secret {
            "<redacted>"
        } else {
            self.value.as_str()
        };
        f.debug_struct("Setting")
            .field("value", &value)
            .field("secret", &self.secret)
            .finish()
    }
}

/// Live string settings.
///
/// `get` returns the value currently in memory. A write is visible to the next
/// `get` without restarting the process. The implementation syncs that memory
/// to its durable source.
#[async_trait]
pub trait SettingsStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Setting>, ServiceError>;

    async fn set(&self, key: &str, value: String, secret: bool) -> Result<(), ServiceError>;
}
