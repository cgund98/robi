use async_trait::async_trait;
use robi_core::ids::WorkspaceId;

/// Removes files derived from a workspace. Today that is only the retired
/// semantic index under `~/.robi/index/<workspace_id>/`.
///
/// The domain performs no I/O, so this is the port and the adapter does the
/// file work — the same split as [`crate::domain::chat_session::data::SessionDataCleaner`].
///
/// Cleanup is best effort: `delete_workspace` logs a failure and still reports
/// success, because the workspace row is already gone.
#[async_trait]
pub trait WorkspaceAssetCleaner: Send + Sync {
    async fn remove_workspace_assets(&self, id: WorkspaceId) -> Result<(), String>;
}
