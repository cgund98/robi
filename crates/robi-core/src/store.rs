//! The persistence port.
//!
//! Incremental `append`/`update` rather than one snapshot write per turn: a
//! snapshot store cannot express approving one call while another is still
//! pending, and it rewrites the whole transcript on every tool completion.

use async_trait::async_trait;

use crate::error::StoreError;
use crate::ids::{MessageId, SessionId, WorkspaceId};
use crate::message::Message;

#[async_trait]
pub trait MessageStore: Send + Sync {
    /// Open a session in a workspace.
    ///
    /// Sync because it allocates a session id and can be answered without I/O.
    fn create_session(&self, workspace: WorkspaceId) -> SessionId;

    /// Whether the store knows this session.
    fn has_session(&self, session: SessionId) -> bool;

    /// The whole transcript, in order.
    async fn messages(&self, session: SessionId) -> Result<Vec<Message>, StoreError>;

    /// One message in the session, looked up by id.
    ///
    /// `Ok(None)` means the session exists and the message does not.
    async fn message(
        &self,
        session: SessionId,
        id: MessageId,
    ) -> Result<Option<Message>, StoreError>;

    /// Add a message to the end of the transcript.
    async fn append(&self, session: SessionId, message: Message) -> Result<(), StoreError>;

    /// Replace the message with the same id.
    ///
    /// Used for status changes on an existing message: a tool call going from
    /// `pending` to `approved`, or gaining its result.
    async fn update(&self, session: SessionId, message: Message) -> Result<(), StoreError>;

    /// Delete `delete` and insert `summary` before what remains, atomically.
    ///
    /// Compaction rewrites the transcript once, after the summary call has
    /// finished. An error leaves the transcript exactly as it was. The summary
    /// takes the front position, so a later transcript read starts with it.
    async fn replace_prefix(
        &self,
        session: SessionId,
        delete: &[MessageId],
        summary: Message,
    ) -> Result<(), StoreError>;
}
