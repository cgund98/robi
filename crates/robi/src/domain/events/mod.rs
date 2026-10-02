//! Loop events as CloudEvents, fanned out in process.
//!
//! No HTTP and no files. `web_api` writes the SSE frames. The composition root
//! hands `FanOutEventSink` to the agent.

mod envelope;
mod fanout;
mod sink;

pub use envelope::{
    EventEnvelope, AGENT_EVENT_TYPES, AWAITING_APPROVAL, MESSAGE_ADDED, MESSAGE_DELTA,
    MESSAGE_UPDATED, SESSION_UPDATED, SOURCE, SPEC_VERSION, TOOL_CALL_UPDATED, TURN_FINISHED,
    TURN_STARTED,
};
pub use fanout::{EventFanOut, EventSubscription, FANOUT_CAPACITY};
pub use sink::FanOutEventSink;
