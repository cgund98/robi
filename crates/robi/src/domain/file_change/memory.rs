//! In-memory baselines for tests that do not open SQLite.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use robi_core::ids::SessionId;

use crate::domain::{
    error::ServiceError,
    file_change::{model::FileBaseline, repo::FileChangeRepository},
};

#[derive(Default)]
pub struct MemoryFileChangeRepository {
    rows: Mutex<BTreeMap<(SessionId, String), FileBaseline>>,
}

impl MemoryFileChangeRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl FileChangeRepository for MemoryFileChangeRepository {
    async fn record_baseline(
        &self,
        session_id: SessionId,
        path: &str,
        baseline: &str,
        created: bool,
    ) -> Result<bool, ServiceError> {
        let mut rows = self.rows.lock().expect("file baselines");
        let key = (session_id, path.to_owned());
        if rows.contains_key(&key) {
            return Ok(false);
        }
        rows.insert(
            key,
            FileBaseline {
                path: path.to_owned(),
                baseline: baseline.to_owned(),
                created,
            },
        );
        Ok(true)
    }

    async fn get_baseline(
        &self,
        session_id: SessionId,
        path: &str,
    ) -> Result<Option<FileBaseline>, ServiceError> {
        let rows = self.rows.lock().expect("file baselines");
        Ok(rows.get(&(session_id, path.to_owned())).cloned())
    }

    async fn list_baselines(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<FileBaseline>, ServiceError> {
        let rows = self.rows.lock().expect("file baselines");
        Ok(rows
            .iter()
            .filter(|((id, _), _)| *id == session_id)
            .map(|(_, baseline)| baseline.clone())
            .collect())
    }

    async fn delete_baseline(&self, session_id: SessionId, path: &str) -> Result<(), ServiceError> {
        let mut rows = self.rows.lock().expect("file baselines");
        rows.remove(&(session_id, path.to_owned()));
        Ok(())
    }

    async fn replace_baseline(
        &self,
        session_id: SessionId,
        path: &str,
        baseline: &str,
    ) -> Result<(), ServiceError> {
        let mut rows = self.rows.lock().expect("file baselines");
        let Some(row) = rows.get_mut(&(session_id, path.to_owned())) else {
            return Err(ServiceError::NotFound(path.to_owned()));
        };
        row.baseline = baseline.to_owned();
        Ok(())
    }
}
