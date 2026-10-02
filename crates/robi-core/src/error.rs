//! Error types.
//!
//! Every error is `Clone`, because a turn outcome travels through the event sink
//! as well as back to the caller.

use crate::ids::SessionId;

/// A failure reported by a `Model` implementation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("the model stream closed before a finished message arrived")]
    StreamClosed,
}

/// A failure reported by a `MessageStore` implementation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("session not found: {0}")]
    SessionNotFound(SessionId),
    #[error("store backend error: {0}")]
    Backend(String),
}

/// A failure of one tool call.
///
/// `NotFound` and `Panicked` are produced by the loop rather than by a tool: the
/// first when the registry has no such tool, the second when a tool's task
/// unwinds.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("the tool panicked")]
    Panicked,
    #[error("the tool timed out")]
    TimedOut,
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

impl ToolError {
    /// A stable discriminant for the structured payload the model reads.
    pub fn kind(&self) -> &'static str {
        match self {
            ToolError::NotFound(_) => "not_found",
            ToolError::InvalidArgs(_) => "invalid_args",
            ToolError::Panicked => "panicked",
            ToolError::TimedOut => "timed_out",
            ToolError::Cancelled => "cancelled",
            ToolError::Failed(_) => "failed",
        }
    }
}

/// A failure that ends a turn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    #[error("reached the iteration cap of {0} model turns")]
    MaxIterations(u32),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A failure of a tool's declaration, at registration time.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("a tool named '{0}' is already registered")]
    DuplicateName(String),
}

/// Why a turn stopped.
///
/// `Paused` is normal control flow, not a failure: in a GUI it is the expected
/// result of a tool call that needs a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    /// The assistant produced a message with no tool calls.
    Complete,
    /// A tool call needs a decision. The transcript records where it stopped.
    Paused,
    /// Cancellation fired. The transcript is left resumable or finished.
    Cancelled,
    Failed(AgentError),
}
