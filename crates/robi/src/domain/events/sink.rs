//! `EventSink` that publishes CloudEvents on the bus.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::event::{Event, EventSink};

use crate::domain::events::{bus::EventBus, envelope::EventEnvelope};

/// Maps a core event and publishes it. A disconnected subscriber is not the
/// loop's problem.
pub struct BusEventSink {
    bus: Arc<EventBus>,
}

impl BusEventSink {
    pub fn new(bus: Arc<EventBus>) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl EventSink for BusEventSink {
    async fn emit(&self, event: Event) {
        log_event(&event);
        self.bus.publish(EventEnvelope::from_core_event(event));
    }
}

fn log_event(event: &Event) {
    match event {
        Event::MessageAdded { session, message } => {
            tracing::info!(%session, %message, "message stored");
        }
        Event::MessageUpdated { session, message } => {
            tracing::info!(%session, %message, "message updated");
        }
        Event::ToolCallUpdated {
            session,
            message,
            call,
        } => {
            tracing::info!(%session, %message, %call, "tool call updated");
        }
        Event::AwaitingApproval { session, call } => {
            tracing::info!(%session, %call, "waiting for approval");
        }
        Event::MessageDelta { .. } | Event::TurnStarted { .. } | Event::TurnFinished { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robi_core::event::Event;
    use robi_core::ids::SessionId;

    use crate::domain::events::envelope::TURN_STARTED;

    #[tokio::test]
    async fn emit_publishes_the_envelope() {
        let bus = Arc::new(EventBus::new());
        let mut subscription = bus.subscribe();
        let session = SessionId::new();
        let sink = BusEventSink::new(Arc::clone(&bus));

        sink.emit(Event::TurnStarted { session }).await;

        let envelope = subscription.recv().await.unwrap();
        assert_eq!(envelope.event_type, TURN_STARTED);
        assert_eq!(envelope.subject, session.to_string());
        assert_eq!(envelope.data["session_id"], session.to_string());
    }
}
