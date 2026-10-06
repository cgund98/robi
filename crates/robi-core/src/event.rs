//! Where the loop reports what it did.
//!
//! Events are coarse signals that persisted state changed, not a second copy of
//! it. Every event is emitted after the store write it describes.

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::TurnOutcome;
use crate::ids::{MessageId, SessionId, ToolCallId};
use crate::model::Delta;

/// Something the loop did.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    TurnStarted {
        session: SessionId,
    },
    MessageAdded {
        session: SessionId,
        message: MessageId,
    },
    MessageUpdated {
        session: SessionId,
        message: MessageId,
    },
    /// A partial message. Carries the id of the message being assembled, which
    /// `ModelStream` fixes before the first delta arrives.
    ///
    /// `delta` is boxed because `Delta::Finished` carries a whole `Message`, and
    /// every other event is small. The box keeps `Event` from growing to fit the
    /// one large payload.
    MessageDelta {
        session: SessionId,
        message: MessageId,
        delta: Box<Delta>,
    },
    ToolCallUpdated {
        session: SessionId,
        message: MessageId,
        call: ToolCallId,
    },
    AwaitingApproval {
        session: SessionId,
        call: ToolCallId,
    },
    TurnFinished {
        session: SessionId,
        outcome: TurnOutcome,
    },
}

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn emit(&self, event: Event);
}

/// Drops every event. For callers that drive the loop and read the transcript.
#[derive(Debug, Default, Clone, Copy)]
pub struct NopSink;

#[async_trait]
impl EventSink for NopSink {
    async fn emit(&self, _event: Event) {}
}

/// Forwards events to a channel.
pub struct ChannelSink {
    tx: mpsc::UnboundedSender<Event>,
}

impl ChannelSink {
    /// Returns the sink and the receiver it feeds.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Event>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }
}

impl std::fmt::Debug for ChannelSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelSink").finish()
    }
}

#[async_trait]
impl EventSink for ChannelSink {
    async fn emit(&self, event: Event) {
        // A closed receiver means nobody is listening any more, which is not the
        // loop's problem: the transcript is already written.
        let _ = self.tx.send(event);
    }
}
