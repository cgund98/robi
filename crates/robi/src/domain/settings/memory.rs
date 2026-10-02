//! In-memory store for tests. The process uses the TOML adapter.

use std::collections::BTreeMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::domain::{
    error::ServiceError,
    settings::store::{Setting, SettingsStore},
};

#[derive(Default)]
pub struct MemorySettingsStore {
    values: Mutex<BTreeMap<String, Setting>>,
}

impl MemorySettingsStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SettingsStore for MemorySettingsStore {
    async fn get(&self, key: &str) -> Result<Option<Setting>, ServiceError> {
        Ok(self.values.lock().await.get(key).cloned())
    }

    async fn set(&self, key: &str, value: String, secret: bool) -> Result<(), ServiceError> {
        self.values
            .lock()
            .await
            .insert(key.to_owned(), Setting { value, secret });
        Ok(())
    }
}
