use chrono::{DateTime, Utc};
use robi_core::ids::{SessionId, WorkspaceId};

/// One conversation in one workspace.
///
/// `title` stays unset until something writes one. The model does that after
/// the first turn, unless create or a rename already supplied one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSession {
    pub id: SessionId,
    pub workspace_id: WorkspaceId,
    pub title: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
}

/// Open a chat session. The adapter mints the id and the timestamps.
///
/// `title` is optional. Absent means the model still has to name the chat
/// session after the first turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateChatSessionCommand {
    pub workspace_id: WorkspaceId,
    pub title: Option<String>,
}

/// Rename a chat session. The workspace stays where it was created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateChatSessionCommand {
    pub id: SessionId,
    pub title: String,
}
