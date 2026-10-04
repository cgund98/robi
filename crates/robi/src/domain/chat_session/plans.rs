use async_trait::async_trait;
use robi_core::ids::SessionId;

/// Removes a session's plan files under `~/.robi/plans/<session_id>`.
///
/// The domain performs no I/O, so this is the port and the adapter does the
/// file work — the same split as `WorkspaceRepository::canonicalize_root`.
///
/// Cleanup is best effort: `delete_chat_session` logs a failure and still
/// reports success, because the chat session row is already gone.
#[async_trait]
pub trait SessionPlanCleaner: Send + Sync {
    async fn remove_session_plans(&self, session: SessionId) -> Result<(), String>;
}
