//! The compressor the loop calls after a tool returns.
//!
//! The loop records an id and does not open a database. A compressor that
//! fails, or that saves nothing, leaves the tool result unchanged.

use async_trait::async_trait;
use serde_json::Value;

/// One tool result, after the tool's own cap and before the core ceiling.
pub struct CompressRequest {
    pub tool: String,
    pub tool_call_id: String,
    pub session_id: String,
    pub result: Value,
}

/// What replaces the tool result in the transcript.
pub struct CompressOutcome {
    /// Equal to the input when this call was a no-op.
    pub result: Value,
    pub original_id: Option<String>,
    /// The pre-compression body, when the caller stores it. The shell
    /// compressor writes the row itself and leaves this empty.
    pub original_bytes: Option<Vec<u8>>,
}

impl CompressOutcome {
    pub fn unchanged(result: Value) -> Self {
        Self {
            result,
            original_id: None,
            original_bytes: None,
        }
    }
}

/// A compressor error never fails the turn. The loop keeps the tool result.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CompressError(pub String);

#[async_trait]
pub trait Compressor: Send + Sync {
    async fn compress(&self, request: CompressRequest) -> Result<CompressOutcome, CompressError>;
}
