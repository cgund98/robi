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
pub const TRANSCRIPT_COMPACTED: &str = "robi.agent.v1.transcript_compacted";
pub const SESSION_SOURCE: &str = "robi/session";
pub const SESSION_CREATED: &str = "robi.session.v1.created";
pub const SESSION_UPDATED: &str = "robi.session.v1.updated";
pub const SESSION_DELETED: &str = "robi.session.v1.deleted";
pub const APP_SOURCE: &str = "robi/app";
pub const APP_SUBJECT: &str = "app";
pub const APP_ERROR: &str = "robi.app.v1.error";
pub const INDEX_PROGRESS: &str = "robi.index.v1.progress";
pub const INDEX_SOURCE: &str = "robi/index";
pub const MCP_STATUS: &str = "robi.mcp.v1.status";
pub const MCP_SOURCE: &str = "robi/mcp";

/// Types a `session_id` stream filter still delivers. They are not about the
/// selected session: another window's list, a process-wide failure, or the
/// start and end of a turn in some other session. Message deltas stay filtered
/// so a background turn does not rewrite the sidebar on every token.
pub const SESSION_FILTER_EXCEPTIONS: &[&str] = &[
    SESSION_CREATED,
    SESSION_UPDATED,
    SESSION_DELETED,
    APP_ERROR,
    TURN_STARTED,
    TURN_FINISHED,
];

/// Workspace-scoped types a stream may deliver when it is filtered to one
/// workspace. The subject is the workspace id.
pub const WORKSPACE_EVENT_TYPES: &[&str] = &[INDEX_PROGRESS, MCP_STATUS];

/// Every agent type the shell asks for. Order is the type map in the design.
pub const AGENT_EVENT_TYPES: &[&str] = &[
    TURN_STARTED,
    MESSAGE_ADDED,
    MESSAGE_UPDATED,
    MESSAGE_DELTA,
    TOOL_CALL_UPDATED,
    AWAITING_APPROVAL,
    TURN_FINISHED,
    TRANSCRIPT_COMPACTED,
    SESSION_CREATED,
    SESSION_UPDATED,
    SESSION_DELETED,
    APP_ERROR,
    INDEX_PROGRESS,
    MCP_STATUS,
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
    /// CloudEvents attributes around a JSON payload. `id` and `time` are new
    /// on every call.
    pub fn from_payload(
        source: impl Into<String>,
        event_type: impl Into<String>,
        subject: impl Into<String>,
        data: impl Serialize,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            specversion: SPEC_VERSION.to_owned(),
            id: Uuid::now_v7(),
            source: source.into(),
            event_type: event_type.into(),
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            subject: subject.into(),
            data: serde_json::to_value(data)?,
        })
    }

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
                data["delta"] = delta_json(*delta);
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

        Self::from_payload(SOURCE, event_type, subject, data).expect("event payload")
    }

    /// A chat session row was stored.
    pub fn session_created(session: SessionId) -> Self {
        session_ref(SESSION_CREATED, session)
    }

    /// A stored field on a chat session changed, including a generated title.
    pub fn session_updated(session: SessionId) -> Self {
        session_ref(SESSION_UPDATED, session)
    }

    /// A chat session was titled or renamed. `title` is the new value, so the
    /// shell updates the list without a refetch.
    pub fn session_title_changed(session: SessionId, title: String) -> Self {
        session_ref_with_title(SESSION_UPDATED, session, Some(title))
    }

    /// The display summary changed. The shell refetches the session.
    pub fn session_turn_display(
        session: SessionId,
        display: crate::domain::chat_session::model::TurnDisplay,
    ) -> Self {
        EventEnvelope::from_payload(
            SESSION_SOURCE,
            SESSION_UPDATED,
            session.to_string(),
            json!({
                "session_id": session.to_string(),
                "turn_display": display.as_str(),
            }),
        )
        .expect("turn display payload")
    }

    /// A chat session row was removed.
    pub fn session_deleted(session: SessionId) -> Self {
        session_ref(SESSION_DELETED, session)
    }

    /// A compact rewrote the transcript. `subject` is the session; the shell
    /// refetches the message list, because rows were deleted.
    pub fn transcript_compacted(session: SessionId, message: MessageId) -> Self {
        Self::from_payload(
            SOURCE,
            TRANSCRIPT_COMPACTED,
            session.to_string(),
            message_ref(session, message),
        )
        .expect("compaction payload")
    }

    /// A failure the shell should show. `subject` is [`APP_SUBJECT`].
    pub fn user_error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self::from_payload(
            APP_SOURCE,
            APP_ERROR,
            APP_SUBJECT,
            json!({ "message": message }),
        )
        .expect("error payload")
    }

    /// Index build progress. `subject` is the workspace id.
    pub fn index_progress(workspace_id: &str, data: Value) -> Self {
        Self::from_payload(INDEX_SOURCE, INDEX_PROGRESS, workspace_id, data).expect("index payload")
    }

    /// MCP servers for one workspace changed. `subject` is the workspace id.
    /// The shell refetches `GET /workspaces/{id}/mcp`.
    pub fn mcp_status(workspace_id: &str) -> Self {
        Self::from_payload(
            MCP_SOURCE,
            MCP_STATUS,
            workspace_id,
            json!({ "workspace_id": workspace_id }),
        )
        .expect("mcp payload")
    }
}

fn session_ref(event_type: &str, session: SessionId) -> EventEnvelope {
    session_ref_with_title(event_type, session, Option::<String>::None)
}

fn session_ref_with_title(
    event_type: &str,
    session: SessionId,
    title: impl Into<Option<String>>,
) -> EventEnvelope {
    let mut data = json!({ "session_id": session.to_string() });
    if let Some(title) = Into::<Option<String>>::into(title) {
        data["title"] = json!(title);
    }
    EventEnvelope::from_payload(SESSION_SOURCE, event_type, session.to_string(), data)
        .expect("session payload")
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
                    delta: Box::new(Delta::Text("hi".into())),
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
                delta: Box::new(delta),
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

    #[test]
    fn from_payload_fills_the_envelope() {
        let envelope = EventEnvelope::from_payload(
            "robi/app",
            "robi.app.v1.error",
            "app",
            json!({ "message": "MCP server demo failed to start" }),
        )
        .unwrap();
        assert_eq!(envelope.specversion, SPEC_VERSION);
        assert_eq!(envelope.source, "robi/app");
        assert_eq!(envelope.event_type, "robi.app.v1.error");
        assert_eq!(envelope.subject, "app");
        assert_eq!(envelope.data["message"], "MCP server demo failed to start");
        assert!(chrono::DateTime::parse_from_rfc3339(&envelope.time).is_ok());
        assert_ne!(envelope.id, Uuid::nil());
    }

    #[test]
    fn transcript_compacted_names_the_session_and_the_summary() {
        let session = SessionId::new();
        let message = MessageId::new();
        let envelope = EventEnvelope::transcript_compacted(session, message);
        assert_eq!(envelope.event_type, TRANSCRIPT_COMPACTED);
        assert_eq!(envelope.source, SOURCE);
        assert_eq!(envelope.subject, session.to_string());
        assert_eq!(
            envelope.data,
            json!({
                "session_id": session.to_string(),
                "message_id": message.to_string(),
            })
        );
    }
}
