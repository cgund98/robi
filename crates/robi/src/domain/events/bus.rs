//! In-process bus. Each subscriber has its own queue. A slow subscriber loses
//! the oldest frame.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::Notify;

use crate::domain::events::envelope::EventEnvelope;

/// Frames retained per subscriber before the oldest is dropped.
pub const BUS_CAPACITY: usize = 1024;

struct Slot {
    queue: Mutex<VecDeque<EventEnvelope>>,
    notify: Notify,
    closed: Mutex<bool>,
}

/// Publishes envelopes to every current subscriber.
///
/// A publish with nobody listening is discarded. That is not an error: the
/// transcript is already written.
pub struct EventBus {
    capacity: usize,
    subscribers: Mutex<Vec<Weak<Slot>>>,
}

/// The read end of one subscription.
///
/// Dropping it unsubscribes. `recv` returns `None` when the bus is dropped.
pub struct EventSubscription {
    slot: Arc<Slot>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::with_capacity(BUS_CAPACITY)
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Clone the envelope into every subscriber. A full queue drops the oldest.
    pub fn publish(&self, envelope: EventEnvelope) {
        let mut subscribers = self.subscribers.lock().expect("event bus subscribers");
        subscribers.retain(|weak| {
            let Some(slot) = weak.upgrade() else {
                return false;
            };
            if *slot.closed.lock().expect("event subscription") {
                return false;
            }
            let mut queue = slot.queue.lock().expect("event queue");
            if queue.len() >= self.capacity {
                queue.pop_front();
            }
            queue.push_back(envelope.clone());
            drop(queue);
            slot.notify.notify_one();
            true
        });
    }

    /// A bounded queue. Drop the subscription to unsubscribe.
    pub fn subscribe(&self) -> EventSubscription {
        let slot = Arc::new(Slot {
            queue: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
            closed: Mutex::new(false),
        });
        self.subscribers
            .lock()
            .expect("event bus subscribers")
            .push(Arc::downgrade(&slot));
        EventSubscription { slot }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EventBus {
    fn drop(&mut self) {
        let subscribers = self.subscribers.lock().expect("event bus subscribers");
        for weak in subscribers.iter() {
            if let Some(slot) = weak.upgrade() {
                *slot.closed.lock().expect("event subscription") = true;
                slot.notify.notify_waiters();
            }
        }
    }
}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        *self.slot.closed.lock().expect("event subscription") = true;
        self.slot.notify.notify_waiters();
    }
}

impl EventSubscription {
    pub async fn recv(&mut self) -> Option<EventEnvelope> {
        loop {
            if *self.slot.closed.lock().expect("event subscription") {
                return self.slot.queue.lock().expect("event queue").pop_front();
            }
            if let Some(envelope) = self.slot.queue.lock().expect("event queue").pop_front() {
                return Some(envelope);
            }
            self.slot.notify.notified().await;
        }
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
        let bus = EventBus::new();
        let mut first = bus.subscribe();
        let mut second = bus.subscribe();

        bus.publish(envelope(1));

        assert_eq!(n_of(&first.recv().await.unwrap()), 1);
        assert_eq!(n_of(&second.recv().await.unwrap()), 1);
    }

    #[tokio::test]
    async fn publish_before_subscribe_is_discarded() {
        let bus = EventBus::new();
        bus.publish(envelope(1));
        let mut subscription = bus.subscribe();
        bus.publish(envelope(2));
        assert_eq!(n_of(&subscription.recv().await.unwrap()), 2);
    }

    #[tokio::test]
    async fn full_queue_drops_the_oldest_and_keeps_the_newest() {
        let bus = EventBus::with_capacity(2);
        let mut subscription = bus.subscribe();

        bus.publish(envelope(1));
        bus.publish(envelope(2));
        bus.publish(envelope(3));

        assert_eq!(n_of(&subscription.recv().await.unwrap()), 2);
        assert_eq!(n_of(&subscription.recv().await.unwrap()), 3);
    }

    #[tokio::test]
    async fn unsubscribe_stops_delivery() {
        let bus = EventBus::new();
        let mut kept = bus.subscribe();
        let dropped = bus.subscribe();
        drop(dropped);

        bus.publish(envelope(7));

        assert_eq!(n_of(&kept.recv().await.unwrap()), 7);
        assert!(bus.subscribers.lock().unwrap().iter().all(|weak| {
            weak.upgrade()
                .is_some_and(|slot| !*slot.closed.lock().unwrap())
        }));
        assert_eq!(
            bus.subscribers
                .lock()
                .unwrap()
                .iter()
                .filter(|weak| weak.strong_count() > 0)
                .count(),
            1
        );
    }
}
