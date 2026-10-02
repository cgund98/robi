//! In-process fan-out. A slow subscriber loses the oldest frame.

use tokio::sync::broadcast;

use crate::domain::events::envelope::EventEnvelope;

/// Frames retained per subscriber before the oldest is dropped.
pub const FANOUT_CAPACITY: usize = 1024;

/// Publishes envelopes to every current subscriber.
///
/// A publish with nobody listening is discarded. That is not an error: the
/// transcript is already written.
pub struct EventFanOut {
    tx: broadcast::Sender<EventEnvelope>,
}

/// The read end of one subscription.
///
/// Dropping it unsubscribes. `recv` returns [`broadcast::error::RecvError::Lagged`]
/// when this subscriber missed frames; those frames are already gone.
pub struct EventSubscription {
    rx: broadcast::Receiver<EventEnvelope>,
}

impl EventFanOut {
    pub fn new() -> Self {
        Self::with_capacity(FANOUT_CAPACITY)
    }

    fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Clone the envelope into every subscriber. A full queue drops the oldest.
    pub fn publish(&self, envelope: EventEnvelope) {
        let _ = self.tx.send(envelope);
    }

    /// A bounded queue. Drop the subscription to unsubscribe.
    pub fn subscribe(&self) -> EventSubscription {
        EventSubscription {
            rx: self.tx.subscribe(),
        }
    }
}

impl Default for EventFanOut {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSubscription {
    pub async fn recv(&mut self) -> Result<EventEnvelope, broadcast::error::RecvError> {
        self.rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn envelope(n: u64) -> EventEnvelope {
        EventEnvelope {
            specversion: "1.0".to_owned(),
            id: Uuid::from_u128(n as u128),
            source: "robi/agent".to_owned(),
            event_type: "robi.agent.v1.turn_started".to_owned(),
            time: "2026-01-01T00:00:00.000Z".to_owned(),
            subject: "session".to_owned(),
            data: json!({ "n": n }),
        }
    }

    fn n_of(envelope: &EventEnvelope) -> u64 {
        envelope.data["n"].as_u64().expect("sequence")
    }

    #[tokio::test]
    async fn delivers_to_two_subscribers() {
        let fanout = EventFanOut::new();
        let mut first = fanout.subscribe();
        let mut second = fanout.subscribe();

        fanout.publish(envelope(1));

        assert_eq!(n_of(&first.recv().await.unwrap()), 1);
        assert_eq!(n_of(&second.recv().await.unwrap()), 1);
    }

    #[tokio::test]
    async fn publish_before_subscribe_is_discarded() {
        let fanout = EventFanOut::new();
        fanout.publish(envelope(1));
        let mut subscription = fanout.subscribe();
        fanout.publish(envelope(2));
        assert_eq!(n_of(&subscription.recv().await.unwrap()), 2);
    }

    #[tokio::test]
    async fn full_queue_drops_the_oldest_and_keeps_the_newest() {
        let fanout = EventFanOut::with_capacity(2);
        let mut subscription = fanout.subscribe();

        fanout.publish(envelope(1));
        fanout.publish(envelope(2));
        fanout.publish(envelope(3));

        assert!(matches!(
            subscription.recv().await,
            Err(broadcast::error::RecvError::Lagged(1))
        ));
        assert_eq!(n_of(&subscription.recv().await.unwrap()), 2);
        assert_eq!(n_of(&subscription.recv().await.unwrap()), 3);
    }

    #[tokio::test]
    async fn unsubscribe_stops_delivery() {
        let fanout = EventFanOut::new();
        let mut kept = fanout.subscribe();
        let dropped = fanout.subscribe();
        drop(dropped);

        fanout.publish(envelope(7));

        assert_eq!(n_of(&kept.recv().await.unwrap()), 7);
        assert_eq!(fanout.tx.receiver_count(), 1);
    }
}
