use async_trait::async_trait;
use robi_core::ids::{SessionId, ToolCallId};

use crate::domain::error::ServiceError;

/// What [`ChatRuntime::submit`] decided.
///
/// `Accepted` means the session actor has the instruction. The model has not
/// run yet. `AwaitingApproval` means the transcript is paused on a tool
/// decision, so the instruction was not queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    Accepted,
    AwaitingApproval,
}

/// The port that serializes instructions for one chat session.
///
/// HTTP checks that the session exists and that the instruction has text, then
/// calls this. The implementation owns the per-session actor.
#[async_trait]
pub trait ChatRuntime: Send + Sync {
    async fn submit(
        &self,
        session: SessionId,
        instruction: String,
        images: Vec<robi_core::message::ImageAttachment>,
    ) -> Result<SubmitOutcome, ServiceError>;

    /// Sessions whose actor is running. Not persisted; idle sessions are absent.
    async fn running_session_ids(&self) -> Vec<SessionId>;

    /// Approve or reject one call, then resume the paused turn.
    ///
    /// `reject` is the reason the model reads. `None` approves the call.
    async fn decide(
        &self,
        session: SessionId,
        call: ToolCallId,
        reject: Option<String>,
    ) -> Result<(), ServiceError>;

    /// Cancel the in-flight turn and drop any instruction that has not started.
    ///
    /// Returns after that session's actor has exited. An idle session is a no-op.
    async fn stop(&self, session: SessionId) -> Result<(), ServiceError>;
}
