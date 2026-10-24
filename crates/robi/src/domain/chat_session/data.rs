use async_trait::async_trait;
use robi_core::ids::SessionId;

/// Removes the data a chat session keeps outside its database row.
///
/// Today that is the session's blob directory under `sessions/<id>`. Plan
/// directories are **not** session data: since S9 they live in the workspace at
/// `.robi/plans/<name>/` and outlive the session that wrote them.
///
/// The domain performs no I/O, so this is the port and the adapter does the file
/// work — the same split as `WorkspaceRepository::canonicalize_root`.
///
/// Cleanup is best effort: `delete_chat_session` logs a failure and still
/// reports success, because the chat session row is already gone.
#[async_trait]
pub trait SessionDataCleaner: Send + Sync {
    async fn remove_session_data(&self, session: SessionId) -> Result<(), String>;
}
