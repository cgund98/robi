use async_trait::async_trait;
use robi_core::ids::{SessionId, WorkspaceId};

use crate::domain::{
    chat_session::model::{ChatSession, CreateChatSessionCommand, UpdateChatSessionCommand},
    error::ServiceError,
};

/// Persistence for chat session metadata.
///
/// `None` from [`ChatSessionRepository::get_chat_session`] means the row is absent. The
/// service turns that into [`ServiceError::NotFound`]. Update and delete report
/// a missing row themselves, because they already know whether a write landed.
#[async_trait]
pub trait ChatSessionRepository: Send + Sync {
    async fn create_chat_session(
        &self,
        command: CreateChatSessionCommand,
    ) -> Result<ChatSession, ServiceError>;

    async fn get_chat_session(&self, id: SessionId) -> Result<Option<ChatSession>, ServiceError>;

    /// Every chat session, or those in `workspace_id` when it is set.
    ///
    /// Order is `last_used_at` descending, then `id` descending.
    async fn list_chat_sessions(
        &self,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<Vec<ChatSession>, ServiceError>;

    async fn update_chat_session(
        &self,
        command: UpdateChatSessionCommand,
    ) -> Result<ChatSession, ServiceError>;

    /// Write `title` only when the stored title is null.
    ///
    /// `Ok(None)` means the row exists and already has a title, so nothing was
    /// written. `updated_at` moves when the write lands. `last_used_at` does not.
    async fn set_title_if_unset(
        &self,
        id: SessionId,
        title: String,
    ) -> Result<Option<ChatSession>, ServiceError>;

    async fn delete_chat_session(&self, id: SessionId) -> Result<(), ServiceError>;
}
