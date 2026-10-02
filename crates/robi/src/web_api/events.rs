//! `GET /api/v1/events/stream`.
//!
//! Subscribe, filter, write frames. The envelope is built in `domain`.

use std::{collections::HashSet, convert::Infallible, time::Duration};

use axum::{
    extract::{RawQuery, State},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::get,
    Router,
};
use futures_util::stream::Stream;
use http::{header::HeaderName, HeaderValue};
use serde::Deserialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    domain::{
        error::ServiceError,
        events::{EventEnvelope, EventSubscription},
    },
    web_api::state::AppState,
};

/// How often an idle stream sends an SSE comment. `EventSource` ignores it.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/events/stream", get(stream_events))
        .with_state(state)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/events/stream",
    params(EventsStreamQuery),
    responses(
        (status = 200, description = "CloudEvents JSON as server-sent events. Each frame is `id`, `event`, and `data`.", content_type = "text/event-stream"),
        (status = 400, description = "session_id is not a UUID")
    )
)]
pub async fn stream_events(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ServiceError> {
    // Axum's `Query` extractor rejects a repeated key and a value that contains
    // `.`. Event types are dotted, and the shell repeats `event_types`.
    let filter = StreamFilter::from_query(parse_query(raw.as_deref()))?;
    let subscription = state.event_fanout.subscribe();
    let stream = event_stream(subscription, filter);
    let response = (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)),
    )
        .into_response();
    Ok(response)
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventsStreamQuery {
    /// Repeated. When non-empty, only these CloudEvents types are sent.
    #[serde(default)]
    pub event_types: Vec<String>,
    /// When set, only envelopes whose subject is this session.
    pub session_id: Option<String>,
}

#[derive(Clone)]
struct StreamFilter {
    event_types: Option<HashSet<String>>,
    session_id: Option<String>,
}

impl StreamFilter {
    fn from_query(query: EventsStreamQuery) -> Result<Self, ServiceError> {
        let event_types = if query.event_types.is_empty() {
            None
        } else {
            Some(query.event_types.into_iter().collect())
        };
        let session_id = match query.session_id.as_deref() {
            None | Some("") => None,
            Some(value) => Some(
                Uuid::parse_str(value)
                    .map_err(|_| ServiceError::BadRequest("session_id must be a UUID".into()))?
                    .to_string(),
            ),
        };
        Ok(Self {
            event_types,
            session_id,
        })
    }

    fn matches(&self, envelope: &EventEnvelope) -> bool {
        if let Some(session_id) = &self.session_id {
            if envelope.subject != *session_id {
                return false;
            }
        }
        if let Some(event_types) = &self.event_types {
            if !event_types.contains(&envelope.event_type) {
                return false;
            }
        }
        true
    }
}

fn parse_query(raw: Option<&str>) -> EventsStreamQuery {
    let mut query = EventsStreamQuery {
        event_types: Vec::new(),
        session_id: None,
    };
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return query;
    };
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match decode_query_component(key).as_str() {
            "event_types" => {
                let value = decode_query_component(value);
                if !value.is_empty() {
                    query.event_types.push(value);
                }
            }
            "session_id" => query.session_id = Some(decode_query_component(value)),
            _ => {}
        }
    }
    query
}

fn decode_query_component(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let raw = input.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        match raw[index] {
            b'+' => {
                bytes.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < raw.len() => {
                match u8::from_str_radix(&input[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        bytes.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        bytes.push(b'%');
                        index += 1;
                    }
                }
            }
            byte => {
                bytes.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn event_stream(
    subscription: EventSubscription,
    filter: StreamFilter,
) -> impl Stream<Item = Result<SseEvent, Infallible>> {
    futures_util::stream::unfold(
        (subscription, filter),
        |(mut subscription, filter)| async move {
            loop {
                match subscription.recv().await {
                    Ok(envelope) => {
                        if !filter.matches(&envelope) {
                            continue;
                        }
                        return Some((Ok(frame(&envelope)), (subscription, filter)));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    )
}

fn frame(envelope: &EventEnvelope) -> SseEvent {
    let data = serde_json::to_string(envelope).expect("an envelope is JSON");
    SseEvent::default()
        .id(envelope.id.to_string())
        .event(envelope.event_type.clone())
        .data(data)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use futures_util::{Stream, StreamExt};
    use http;
    use robi_core::error::StoreError;
    use robi_core::ids::{MessageId, SessionId, WorkspaceId};
    use robi_core::message::Message;
    use robi_core::store::MessageStore;
    use uuid::Uuid;

    use crate::domain::{
        chat_message::{runtime::ChatRuntime, service::ChatMessageService, SubmitOutcome},
        chat_session::{
            model::{ChatSession, CreateChatSessionCommand, UpdateChatSessionCommand},
            repo::ChatSessionRepository,
            service::ChatSessionService,
        },
        error::ServiceError,
        events::{EventEnvelope, EventFanOut, TURN_STARTED},
        settings::{memory::MemorySettingsStore, store::SettingsStore, SettingsService},
    };
    use crate::web_api::state::AppState;

    struct Unused;

    #[async_trait]
    impl ChatSessionRepository for Unused {
        async fn create_chat_session(
            &self,
            _command: CreateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            unreachable!("events stream does not touch chat sessions")
        }

        async fn get_chat_session(
            &self,
            _id: SessionId,
        ) -> Result<Option<ChatSession>, ServiceError> {
            unreachable!("events stream does not touch chat sessions")
        }

        async fn list_chat_sessions(
            &self,
            _workspace_id: Option<WorkspaceId>,
        ) -> Result<Vec<ChatSession>, ServiceError> {
            unreachable!("events stream does not touch chat sessions")
        }

        async fn update_chat_session(
            &self,
            _command: UpdateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            unreachable!("events stream does not touch chat sessions")
        }

        async fn set_title_if_unset(
            &self,
            _id: SessionId,
            _title: String,
        ) -> Result<Option<ChatSession>, ServiceError> {
            unreachable!("events stream does not touch chat sessions")
        }

        async fn delete_chat_session(&self, _id: SessionId) -> Result<(), ServiceError> {
            unreachable!("events stream does not touch chat sessions")
        }
    }

    #[async_trait]
    impl ChatRuntime for Unused {
        async fn submit(
            &self,
            _session: SessionId,
            _instruction: String,
        ) -> Result<SubmitOutcome, ServiceError> {
            unreachable!("events stream does not submit")
        }

        async fn running_session_ids(&self) -> Vec<SessionId> {
            Vec::new()
        }

        async fn decide(
            &self,
            _session: SessionId,
            _call: robi_core::ids::ToolCallId,
            _reject: Option<String>,
        ) -> Result<(), ServiceError> {
            unreachable!("events stream does not settle tool calls")
        }
    }

    #[async_trait]
    impl MessageStore for Unused {
        fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
            unreachable!("events stream does not touch the transcript")
        }

        fn has_session(&self, _session: SessionId) -> bool {
            unreachable!("events stream does not touch the transcript")
        }

        async fn messages(&self, _session: SessionId) -> Result<Vec<Message>, StoreError> {
            unreachable!("events stream does not touch the transcript")
        }

        async fn message(
            &self,
            _session: SessionId,
            _id: MessageId,
        ) -> Result<Option<Message>, StoreError> {
            unreachable!("events stream does not touch the transcript")
        }

        async fn append(&self, _session: SessionId, _message: Message) -> Result<(), StoreError> {
            unreachable!("events stream does not touch the transcript")
        }

        async fn update(&self, _session: SessionId, _message: Message) -> Result<(), StoreError> {
            unreachable!("events stream does not touch the transcript")
        }
    }

    fn test_state(fanout: Arc<EventFanOut>) -> AppState {
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(Unused),
            workspaces: Arc::new(crate::domain::workspace::repo::AnyWorkspace),
        });
        let settings: Arc<dyn SettingsStore> = Arc::new(MemorySettingsStore::new());
        AppState {
            workspace_service: Arc::new(crate::domain::workspace::service::WorkspaceService {
                repository: Arc::new(crate::domain::workspace::repo::AnyWorkspace),
            }),
            chat_session_service: Arc::clone(&sessions),
            chat_message_service: Arc::new(ChatMessageService {
                sessions,
                runtime: Arc::new(Unused),
                store: Arc::new(Unused),
            }),
            settings_service: Arc::new(SettingsService { store: settings }),
            event_fanout: fanout,
        }
    }

    fn sample(subject: &str, event_type: &str) -> EventEnvelope {
        EventEnvelope {
            specversion: "1.0".to_owned(),
            id: Uuid::now_v7(),
            source: "robi/agent".to_owned(),
            event_type: event_type.to_owned(),
            time: "2026-01-01T00:00:00.000Z".to_owned(),
            subject: subject.to_owned(),
            data: serde_json::json!({ "session_id": subject }),
        }
    }

    async fn open_stream(state: AppState, query: &str) -> reqwest::Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = crate::web_api::router(state);
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        reqwest::Client::new()
            .get(format!("http://{addr}/api/v1/events/stream{query}"))
            .send()
            .await
            .unwrap()
    }

    async fn read_frame(
        body: &mut (impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin),
    ) -> String {
        let mut buf = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            if buf.windows(2).any(|window| window == b"\n\n") {
                break;
            }
            let chunk = tokio::time::timeout_at(deadline, body.next())
                .await
                .expect("timed out waiting for an SSE frame")
                .expect("stream closed")
                .expect("body error");
            buf.extend_from_slice(&chunk);
        }
        String::from_utf8(buf).expect("frame is utf-8")
    }

    fn field<'a>(frame: &'a str, name: &str) -> &'a str {
        frame
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}: ")))
            .unwrap_or_else(|| panic!("missing {name} in {frame}"))
    }

    #[tokio::test]
    async fn publishes_one_sse_frame() {
        let fanout = Arc::new(EventFanOut::new());
        let response = open_stream(test_state(Arc::clone(&fanout)), "").await;

        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            response.headers().get(http::header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );
        assert_eq!(response.headers().get("x-accel-buffering").unwrap(), "no");

        let envelope = sample(&SessionId::new().to_string(), TURN_STARTED);
        fanout.publish(envelope.clone());

        let mut body = response.bytes_stream();
        let frame = read_frame(&mut body).await;
        let data: EventEnvelope = serde_json::from_str(field(&frame, "data")).unwrap();
        assert_eq!(field(&frame, "id"), data.id.to_string());
        assert_eq!(field(&frame, "event"), TURN_STARTED);
        assert_eq!(data, envelope);
    }

    #[tokio::test]
    async fn session_id_query_drops_a_different_subject() {
        let fanout = Arc::new(EventFanOut::new());
        let wanted = SessionId::new().to_string();
        let other = SessionId::new().to_string();
        let response = open_stream(
            test_state(Arc::clone(&fanout)),
            &format!("?session_id={wanted}"),
        )
        .await;
        assert!(response.status().is_success());

        fanout.publish(sample(&other, TURN_STARTED));
        let kept = sample(&wanted, TURN_STARTED);
        fanout.publish(kept.clone());

        let mut body = response.bytes_stream();
        let frame = read_frame(&mut body).await;
        let data: EventEnvelope = serde_json::from_str(field(&frame, "data")).unwrap();
        assert_eq!(data.subject, wanted);
        assert_eq!(data, kept);

        let pending = tokio::time::timeout(Duration::from_millis(200), body.next()).await;
        assert!(pending.is_err(), "the other subject was written");
    }

    #[tokio::test]
    async fn event_types_query_drops_other_types() {
        let fanout = Arc::new(EventFanOut::new());
        let session = SessionId::new().to_string();
        let response = open_stream(
            test_state(Arc::clone(&fanout)),
            &format!("?event_types={TURN_STARTED}&event_types=robi.agent.v1.message_added"),
        )
        .await;
        assert!(response.status().is_success(), "{}", response.status());

        fanout.publish(sample(&session, "robi.agent.v1.tool_call_updated"));
        let kept = sample(&session, TURN_STARTED);
        fanout.publish(kept.clone());

        let mut body = response.bytes_stream();
        let frame = read_frame(&mut body).await;
        let data: EventEnvelope = serde_json::from_str(field(&frame, "data")).unwrap();
        assert_eq!(data, kept);
    }

    #[tokio::test]
    async fn bad_session_id_is_rejected() {
        let fanout = Arc::new(EventFanOut::new());
        let response = open_stream(test_state(fanout), "?session_id=nope").await;
        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    }
}
