//! CloudEvents 1.0 envelope for a core `Event`.
//!
//! Serialization lives here so `robi-core` never names CloudEvents.

use chrono::{SecondsFormat, Utc};
use robi_core::error::TurnOutcome;
use robi_core::event::Event;
use robi_core::ids::{MessageId, SessionId};
use robi_core::model::Delta;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub const SPEC_VERSION: &str = "1.0";
pub const SOURCE: &str = "robi/agent";

pub const TURN_STARTED: &str = "robi.agent.v1.turn_started";
pub const MESSAGE_ADDED: &str = "robi.agent.v1.message_added";
pub const MESSAGE_UPDATED: &str = "robi.agent.v1.message_updated";
pub const MESSAGE_DELTA: &str = "robi.agent.v1.message_delta";
pub const TOOL_CALL_UPDATED: &str = "robi.agent.v1.tool_call_updated";
pub const AWAITING_APPROVAL: &str = "robi.agent.v1.awaiting_approval";
pub const TURN_FINISHED: &str = "robi.agent.v1.turn_finished";
pub const SESSION_UPDATED: &str = "robi.agent.v1.session_updated";

/// Every agent type the shell asks for. Order is the type map in the design.
pub const AGENT_EVENT_TYPES: &[&str] = &[
    TURN_STARTED,
    MESSAGE_ADDED,
    MESSAGE_UPDATED,
    MESSAGE_DELTA,
    TOOL_CALL_UPDATED,
    AWAITING_APPROVAL,
    TURN_FINISHED,
    SESSION_UPDATED,
];

/// One event on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub specversion: String,
    pub id: Uuid,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    /// RFC 3339 UTC.
    pub time: String,
    pub subject: String,
    pub data: Value,
}

impl EventEnvelope {
    /// Wrap a loop event. `id` and `time` are new on every call.
    pub fn from_core_event(event: Event) -> Self {
        let (event_type, subject, data) = match event {
            Event::TurnStarted { session } => (
                TURN_STARTED,
                session.to_string(),
                json!({ "session_id": session.to_string() }),
            ),
            Event::MessageAdded { session, message } => (
                MESSAGE_ADDED,
                session.to_string(),
                message_ref(session, message),
            ),
            Event::MessageUpdated { session, message } => (
                MESSAGE_UPDATED,
                session.to_string(),
                message_ref(session, message),
            ),
            Event::MessageDelta {
                session,
                message,
                delta,
            } => (MESSAGE_DELTA, session.to_string(), {
                let mut data = message_ref(session, message);
                data["delta"] = delta_json(delta);
                data
            }),
            Event::ToolCallUpdated {
                session,
                message,
                call,
            } => (TOOL_CALL_UPDATED, session.to_string(), {
                let mut data = message_ref(session, message);
                data["tool_call_id"] = json!(call.to_string());
                data
            }),
            Event::AwaitingApproval { session, call } => (
                AWAITING_APPROVAL,
                session.to_string(),
                json!({
                    "session_id": session.to_string(),
                    "tool_call_id": call.to_string(),
                }),
            ),
            Event::TurnFinished { session, outcome } => (
                TURN_FINISHED,
                session.to_string(),
                json!({
                    "session_id": session.to_string(),
                    "outcome": outcome_json(outcome),
                }),
            ),
        };

        Self {
            specversion: SPEC_VERSION.to_owned(),
            id: Uuid::now_v7(),
            source: SOURCE.to_owned(),
            event_type: event_type.to_owned(),
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            subject,
            data,
        }
    }

    /// A chat session's stored title changed. Not a loop event: the runtime
    /// publishes this after the title row is written.
    pub fn session_updated(session: SessionId, title: &str) -> Self {
        Self {
            specversion: SPEC_VERSION.to_owned(),
            id: Uuid::now_v7(),
            source: SOURCE.to_owned(),
            event_type: SESSION_UPDATED.to_owned(),
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            subject: session.to_string(),
            data: json!({
                "session_id": session.to_string(),
                "title": title,
            }),
        }
    }
}

fn message_ref(session: SessionId, message: MessageId) -> Value {
    json!({
        "session_id": session.to_string(),
        "message_id": message.to_string(),
    })
}

fn outcome_json(outcome: TurnOutcome) -> Value {
    match outcome {
        TurnOutcome::Complete => json!({ "kind": "complete" }),
        TurnOutcome::Paused => json!({ "kind": "paused" }),
        TurnOutcome::Cancelled => json!({ "kind": "cancelled" }),
        TurnOutcome::Failed(error) => json!({ "kind": "failed", "message": error.to_string() }),
    }
}

/// `Finished` carries no message body. The UI refetches the row.
fn delta_json(delta: Delta) -> Value {
    match delta {
        Delta::Text(text) => json!({ "kind": "text", "text": text }),
        Delta::Reasoning(text) => json!({ "kind": "reasoning", "text": text }),
        Delta::ToolCallStart { index, id, name } => json!({
            "kind": "tool_call_start",
            "index": index,
            "id": id.to_string(),
            "name": name,
        }),
        Delta::ToolCallArgs { index, fragment } => json!({
            "kind": "tool_call_args",
            "index": index,
            "fragment": fragment,
        }),
        Delta::ToolCallEnd { index } => json!({ "kind": "tool_call_end", "index": index }),
        Delta::Usage(usage) => json!({ "kind": "usage", "usage": usage }),
        Delta::Finished(_message) => json!({ "kind": "finished" }),
        Delta::Failed(error) => json!({ "kind": "failed", "message": error.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robi_core::error::{AgentError, ModelError};
    use robi_core::ids::ToolCallId;
    use robi_core::message::{Message, Usage};

    #[test]
    fn from_core_event_covers_every_variant() {
        let session = SessionId::new();
        let message = MessageId::new();
        let call = ToolCallId::new();
        let finished_body = "secret-finished-body";

        let cases = vec![
            (
                Event::TurnStarted { session },
                TURN_STARTED,
                json!({ "session_id": session.to_string() }),
            ),
            (
                Event::MessageAdded { session, message },
                MESSAGE_ADDED,
                message_ref(session, message),
            ),
            (
                Event::MessageUpdated { session, message },
                MESSAGE_UPDATED,
                message_ref(session, message),
            ),
            (
                Event::MessageDelta {
                    session,
                    message,
                    delta: Delta::Text("hi".into()),
                },
                MESSAGE_DELTA,
                json!({
                    "session_id": session.to_string(),
                    "message_id": message.to_string(),
                    "delta": { "kind": "text", "text": "hi" },
                }),
            ),
            (
                Event::ToolCallUpdated {
                    session,
                    message,
                    call,
                },
                TOOL_CALL_UPDATED,
                json!({
                    "session_id": session.to_string(),
                    "message_id": message.to_string(),
                    "tool_call_id": call.to_string(),
                }),
            ),
            (
                Event::AwaitingApproval { session, call },
                AWAITING_APPROVAL,
                json!({
                    "session_id": session.to_string(),
                    "tool_call_id": call.to_string(),
                }),
            ),
            (
                Event::TurnFinished {
                    session,
                    outcome: TurnOutcome::Complete,
                },
                TURN_FINISHED,
                json!({
                    "session_id": session.to_string(),
                    "outcome": { "kind": "complete" },
                }),
            ),
        ];

        for (event, event_type, data) in cases {
            let envelope = EventEnvelope::from_core_event(event);
            assert_eq!(envelope.specversion, SPEC_VERSION);
            assert_eq!(envelope.source, SOURCE);
            assert_eq!(envelope.event_type, event_type);
            assert_eq!(envelope.subject, session.to_string());
            assert_eq!(envelope.data, data);
            assert!(chrono::DateTime::parse_from_rfc3339(&envelope.time).is_ok());
        }

        let deltas = [
            (
                Delta::Reasoning("why".into()),
                json!({ "kind": "reasoning", "text": "why" }),
            ),
            (
                Delta::ToolCallStart {
                    index: 1,
                    id: call,
                    name: "read".into(),
                },
                json!({
                    "kind": "tool_call_start",
                    "index": 1,
                    "id": call.to_string(),
                    "name": "read",
                }),
            ),
            (
                Delta::ToolCallArgs {
                    index: 1,
                    fragment: "{\"a\":".into(),
                },
                json!({ "kind": "tool_call_args", "index": 1, "fragment": "{\"a\":" }),
            ),
            (
                Delta::ToolCallEnd { index: 1 },
                json!({ "kind": "tool_call_end", "index": 1 }),
            ),
            (
                Delta::Usage(Usage {
                    input: 3,
                    output: 4,
                    cached: 1,
                }),
                json!({ "kind": "usage", "usage": { "input": 3, "output": 4, "cached": 1 } }),
            ),
            (
                Delta::Finished(Message::assistant(finished_body)),
                json!({ "kind": "finished" }),
            ),
            (
                Delta::Failed(ModelError::StreamClosed),
                json!({
                    "kind": "failed",
                    "message": "the model stream closed before a finished message arrived",
                }),
            ),
        ];

        for (delta, expected) in deltas {
            let envelope = EventEnvelope::from_core_event(Event::MessageDelta {
                session,
                message,
                delta,
            });
            assert_eq!(envelope.data["delta"], expected);
            assert!(!envelope.data.to_string().contains(finished_body));
        }

        let outcomes = [
            (TurnOutcome::Paused, json!({ "kind": "paused" })),
            (TurnOutcome::Cancelled, json!({ "kind": "cancelled" })),
            (
                TurnOutcome::Failed(AgentError::MaxIterations(2)),
                json!({
                    "kind": "failed",
                    "message": "reached the iteration cap of 2 model turns",
                }),
            ),
        ];
        for (outcome, expected) in outcomes {
            let envelope = EventEnvelope::from_core_event(Event::TurnFinished { session, outcome });
            assert_eq!(envelope.data["outcome"], expected);
        }
    }
}
