//! Loop events as CloudEvents, published on an in-process bus.
//!
//! No HTTP and no files. `web_api` writes the SSE frames. The composition root
//! hands `BusEventSink` to the agent.

mod bus;
mod envelope;
mod sink;

pub use bus::{EventBus, EventSubscription, BUS_CAPACITY};
pub use envelope::{
    EventEnvelope, AGENT_EVENT_TYPES, APP_ERROR, APP_SOURCE, APP_SUBJECT, AWAITING_APPROVAL,
    INDEX_PROGRESS, INDEX_SOURCE, MCP_SOURCE, MCP_STATUS, MESSAGE_ADDED, MESSAGE_DELTA,
    MESSAGE_UPDATED, SESSION_CREATED, SESSION_DELETED, SESSION_FILTER_EXCEPTIONS, SESSION_SOURCE,
    SESSION_UPDATED, SOURCE, SPEC_VERSION, TOOL_CALL_UPDATED, TRANSCRIPT_COMPACTED, TURN_FINISHED,
    TURN_STARTED, WORKSPACE_EVENT_TYPES,
};
pub use sink::BusEventSink;
