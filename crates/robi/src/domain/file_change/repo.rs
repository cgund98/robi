use async_trait::async_trait;
use robi_core::ids::SessionId;

use crate::domain::{error::ServiceError, file_change::model::FileBaseline};

/// Persistence for the pre-edit body of each path a chat session changes.
///
/// The first insert wins. A later call for the same path does not replace it.
#[async_trait]
pub trait FileChangeRepository: Send + Sync {
    /// Store `baseline` when this session has no row for `path` yet.
    ///
    /// `Ok(true)` means this call inserted the row.
    async fn record_baseline(
        &self,
        session_id: SessionId,
        path: &str,
        baseline: &str,
        created: bool,
    ) -> Result<bool, ServiceError>;

    async fn get_baseline(
        &self,
        session_id: SessionId,
        path: &str,
    ) -> Result<Option<FileBaseline>, ServiceError>;

    /// Every baseline for the session, ordered by path.
    async fn list_baselines(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<FileBaseline>, ServiceError>;

    /// Remove one path. A missing row is success.
    async fn delete_baseline(&self, session_id: SessionId, path: &str) -> Result<(), ServiceError>;
}
