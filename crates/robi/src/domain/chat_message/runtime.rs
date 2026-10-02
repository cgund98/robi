use async_trait::async_trait;
use robi_core::ids::SessionId;

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
    ) -> Result<SubmitOutcome, ServiceError>;

    /// Sessions whose actor is running. Not persisted; idle sessions are absent.
    async fn running_session_ids(&self) -> Vec<SessionId>;
}
