//! The model port and the delta vocabulary.
//!
//! The loop consumes a stream from the first milestone, even though no provider
//! streams behind it yet. Making the call blocking and changing it later would
//! touch every call site, and the delta stream is the contract a UI is built
//! against.

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::ModelError;
use crate::ids::{MessageId, ToolCallId};
use crate::message::{Message, Usage};

/// One piece of a model turn, as it arrives.
///
/// `index` is stream-local: it locates a tool call while its arguments are still
/// arriving. Mapping an index to the provider-assigned call id is adapter work,
/// and the loop never sees an index.
#[derive(Debug, Clone, PartialEq)]
pub enum Delta {
    Text(String),
    Reasoning(String),
    ToolCallStart {
        index: usize,
        id: ToolCallId,
        name: String,
    },
    ToolCallArgs {
        index: usize,
        fragment: String,
    },
    ToolCallEnd {
        index: usize,
    },
    Usage(Usage),
    /// The adapter's assembled message. The loop stamps it with the stream's
    /// message id, so the transcript id and the delta events agree.
    Finished(Message),
    Failed(ModelError),
}

/// A model turn in progress.
///
/// Carries the identity of the message it is assembling, so a delta event can
/// name its message before the finished message exists.
pub struct ModelStream {
    id: MessageId,
    rx: mpsc::Receiver<Delta>,
}

impl ModelStream {
    /// Wrap a delta channel, minting the id this turn's message will carry.
    pub fn new(rx: mpsc::Receiver<Delta>) -> Self {
        Self {
            id: MessageId::new(),
            rx,
        }
    }

    /// Wrap a delta channel with an id chosen by the caller.
    pub fn with_message_id(id: MessageId, rx: mpsc::Receiver<Delta>) -> Self {
        Self { id, rx }
    }

    /// The id the finished message will carry.
    pub fn message_id(&self) -> MessageId {
        self.id
    }

    /// The next delta, or `None` once the adapter stops sending.
    pub async fn next(&mut self) -> Option<Delta> {
        self.rx.recv().await
    }
}

impl From<mpsc::Receiver<Delta>> for ModelStream {
    fn from(rx: mpsc::Receiver<Delta>) -> Self {
        Self::new(rx)
    }
}

#[async_trait]
pub trait Model: Send + Sync {
    /// Start one model turn over the transcript.
    async fn generate(
        &self,
        transcript: &[Message],
        cancel: CancellationToken,
    ) -> Result<ModelStream, ModelError>;
}

/// A model that always fails. Useful as a placeholder before M1.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableModel;

#[async_trait]
impl Model for UnavailableModel {
    async fn generate(
        &self,
        _transcript: &[Message],
        _cancel: CancellationToken,
    ) -> Result<ModelStream, ModelError> {
        Err(ModelError::Provider(
            "no provider is configured; this build has no model adapter".to_owned(),
        ))
    }
}
