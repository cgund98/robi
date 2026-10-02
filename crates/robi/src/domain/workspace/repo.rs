use async_trait::async_trait;
use robi_core::ids::WorkspaceId;

use crate::domain::{error::ServiceError, workspace::model::Workspace};

/// Persistence for workspace roots.
///
/// Canonicalizing a path is I/O, so it lives here rather than in the service.
/// `None` from a get means the row is absent.
#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    /// Resolve `root` to one absolute directory path.
    ///
    /// A missing path, a file, or a path that does not resolve is
    /// [`ServiceError::BadRequest`].
    async fn canonicalize_root(&self, root: &str) -> Result<String, ServiceError>;

    async fn get_workspace(&self, id: WorkspaceId) -> Result<Option<Workspace>, ServiceError>;

    async fn get_workspace_by_root(&self, root: &str) -> Result<Option<Workspace>, ServiceError>;

    /// Insert a row. A duplicate `root` is [`ServiceError::Conflict`].
    async fn insert_workspace(&self, root: &str, name: &str) -> Result<Workspace, ServiceError>;

    /// Every workspace, `created_at` descending, then `id` descending.
    async fn list_workspaces(&self) -> Result<Vec<Workspace>, ServiceError>;

    async fn delete_workspace(&self, id: WorkspaceId) -> Result<(), ServiceError>;
}

/// A repository that reports every id as present. Tests that are not about
/// workspace membership use it so session creates still pass the existence check.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct AnyWorkspace;

#[cfg(test)]
#[async_trait]
impl WorkspaceRepository for AnyWorkspace {
    async fn canonicalize_root(&self, root: &str) -> Result<String, ServiceError> {
        Ok(root.to_string())
    }

    async fn get_workspace(&self, id: WorkspaceId) -> Result<Option<Workspace>, ServiceError> {
        Ok(Some(Workspace {
            id,
            name: "any".to_string(),
            root: "/any".to_string(),
            created_at: chrono::Utc::now(),
        }))
    }

    async fn get_workspace_by_root(&self, _root: &str) -> Result<Option<Workspace>, ServiceError> {
        Ok(None)
    }

    async fn insert_workspace(&self, root: &str, name: &str) -> Result<Workspace, ServiceError> {
        Ok(Workspace {
            id: WorkspaceId::new(),
            name: name.to_string(),
            root: root.to_string(),
            created_at: chrono::Utc::now(),
        })
    }

    async fn list_workspaces(&self) -> Result<Vec<Workspace>, ServiceError> {
        Ok(Vec::new())
    }

    async fn delete_workspace(&self, id: WorkspaceId) -> Result<(), ServiceError> {
        Err(ServiceError::NotFound(id.to_string()))
    }
}
