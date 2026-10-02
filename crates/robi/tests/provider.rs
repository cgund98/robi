//! The adapter against a fake provider that speaks the real wire format.
//!
//! Every case here runs over loopback: no live endpoint, no credential. The server
//! is scripted per request, so chunk splitting, timeouts, retries, and cancellation
//! are deterministic.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use bytes::Bytes;
use robi::providers::{build_model, ApiKey, ModelId, ProviderSettings, RetryPolicy};
use robi_core::agent::Agent;
use robi_core::config::LoopConfig;
use robi_core::error::StoreError;
use robi_core::error::TurnOutcome;
use robi_core::event::NopSink;
use robi_core::ids::{MessageId, SessionId, WorkspaceId};
use robi_core::message::{Message, Role};
use robi_core::model::{Delta, Model, ModelStream};
use robi_core::store::MessageStore;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRegistry};
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// The fake provider
// ---------------------------------------------------------------------------

/// What the fake server answers with.
#[derive(Clone)]
enum Reply {
    /// A 200 with an SSE body. Each payload becomes one `data:` line.
    Sse {
        payloads: Vec<String>,
        /// Applied before every chunk after the first, to open a gap.
        gap: Duration,
        /// Applied before the response is returned, to delay the headers.
        ///
        /// A delay here is the only way to keep a request in flight: once headers
        /// arrive, `generate` has returned and the caller is awaiting a chunk.
        headers_delay: Duration,
    },
    /// A non-2xx status with a JSON body.
    Error {
        status: u16,
        body: String,
        headers: Vec<(&'static str, String)>,
    },
}

/// One request the fake server received.
#[derive(Clone, Debug)]
struct Recorded {
    path: String,
    headers: HeaderMap,
    body: serde_json::Value,
}

#[derive(Clone, Default)]
struct Fake {
    replies: Arc<Mutex<Vec<Reply>>>,
    cursor: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl Fake {
    fn scripted(replies: Vec<Reply>) -> Self {
        Self {
            replies: Arc::new(Mutex::new(replies)),
            cursor: Arc::new(AtomicUsize::new(0)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The next scripted reply. Once the script runs out the last one repeats, so a
    /// test that only cares about a 500 can script it once.
    fn next_reply(&self) -> Reply {
        let replies = self.replies.lock().expect("the script is not poisoned");
        if replies.is_empty() {
            return Reply::Error {
                status: 500,
                body: "no reply scripted".to_owned(),
                headers: Vec::new(),
            };
        }
        let index = self.cursor.fetch_add(1, Ordering::SeqCst);
        replies[index.min(replies.len() - 1)].clone()
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests
            .lock()
            .expect("the log is not poisoned")
            .clone()
    }

    fn request_count(&self) -> usize {
        self.requests.lock().expect("the log is not poisoned").len()
    }
}

async fn handle(State(fake): State<Fake>, headers: HeaderMap, body: Bytes) -> Response {
    fake.requests
        .lock()
        .expect("the log is not poisoned")
        .push(Recorded {
            path: "/chat/completions".to_owned(),
            headers: headers.clone(),
            body: serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        });

    match fake.next_reply() {
        Reply::Error {
            status,
            body,
            headers,
        } => {
            let mut response = Response::new(Body::from(body));
            *response.status_mut() =
                StatusCode::from_u16(status).expect("the script uses a real status");
            for (name, value) in headers {
                response.headers_mut().insert(
                    axum::http::HeaderName::from_static(name),
                    value.parse().expect("a valid header value"),
                );
            }
            response
        }
        Reply::Sse {
            payloads,
            gap,
            headers_delay,
        } => {
            if !headers_delay.is_zero() {
                tokio::time::sleep(headers_delay).await;
            }
            let stream = futures_util::stream::unfold(
                (payloads, 0usize, gap),
                |(payloads, index, gap)| async move {
                    if index >= payloads.len() {
                        return None;
                    }
                    // Only between chunks, so the first one is never delayed.
                    if index > 0 && !gap.is_zero() {
                        tokio::time::sleep(gap).await;
                    }
                    let chunk = format!("data: {}\n\n", payloads[index]);
                    Some((
                        Ok::<_, std::convert::Infallible>(Bytes::from(chunk)),
                        (payloads, index + 1, gap),
                    ))
                },
            );
            let mut response = Response::new(Body::from_stream(stream));
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                "text/event-stream".parse().expect("a valid content type"),
            );
            response
        }
    }
}

/// Start the fake server, returning its base URL.
async fn spawn(fake: Fake) -> String {
    let app = Router::new()
        .route("/chat/completions", post(handle))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("a bound address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{address}")
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn settings(base_url: String) -> ProviderSettings {
    let mut settings =
        ProviderSettings::opencode_go(ApiKey::new("test-key"), ModelId::new("glm-5.3"));
    settings.base_url = base_url;
    // Keep the retry tests fast; the policy shape is what matters here.
    settings.retry = RetryPolicy {
        max_attempts: 3,
        base: Duration::from_millis(10),
        cap: Duration::from_millis(50),
    };
    settings
}

fn build(base_url: String, tools: Arc<ToolRegistry>) -> Arc<dyn Model> {
    build_model(settings(base_url), tools).expect("the model builds")
}

/// Drain a stream, returning every delta and the finished message.
async fn drain(mut stream: ModelStream) -> (Vec<Delta>, Option<Message>) {
    let mut deltas = Vec::new();
    let mut finished = None;
    while let Some(delta) = stream.next().await {
        if let Delta::Finished(message) = &delta {
            finished = Some(message.clone());
        }
        deltas.push(delta);
    }
    (deltas, finished)
}

fn text_stream(text: &str) -> Reply {
    Reply::Sse {
        payloads: vec![
            format!(r#"{{"choices":[{{"delta":{{"content":"{text}"}}}}]}}"#),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#.to_owned(),
            "[DONE]".to_owned(),
        ],
        gap: Duration::ZERO,
        headers_delay: Duration::ZERO,
    }
}

fn api_key() -> String {
    "test-key".to_owned()
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_turn_streams_text_and_finishes() {
    let fake = Fake::scripted(vec![Reply::Sse {
        payloads: vec![
            r#"{"choices":[{"delta":{"content":"Hel"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{"content":"lo"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#.to_owned(),
            "[DONE]".to_owned(),
        ],
        gap: Duration::ZERO,
        headers_delay: Duration::ZERO,
    }]);
    let base = spawn(fake.clone()).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let stream = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .expect("the request is accepted");
    let (deltas, finished) = drain(stream).await;

    assert!(deltas.contains(&Delta::Text("Hel".to_owned())));
    assert!(deltas.contains(&Delta::Text("lo".to_owned())));
    let message = finished.expect("a finished message");
    assert_eq!(message.content, "Hello");
    assert_eq!(message.role, Role::Assistant);
}

#[tokio::test]
async fn reasoning_deltas_reach_the_loop_and_stay_out_of_the_message() {
    let fake = Fake::scripted(vec![Reply::Sse {
        payloads: vec![
            r#"{"choices":[{"delta":{"reasoning_content":"considering"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{"content":"answer"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#.to_owned(),
            "[DONE]".to_owned(),
        ],
        gap: Duration::ZERO,
        headers_delay: Duration::ZERO,
    }]);
    let base = spawn(fake).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let stream = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .expect("the request is accepted");
    let (deltas, finished) = drain(stream).await;

    assert!(deltas.contains(&Delta::Reasoning("considering".to_owned())));
    assert_eq!(finished.expect("a message").content, "answer");
}

#[tokio::test]
async fn the_request_carries_the_agent_session_and_the_credential() {
    let fake = Fake::scripted(vec![text_stream("ok")]);
    let base = spawn(fake.clone()).await;
    let model = build(base, Arc::new(ToolRegistry::new()));
    let session = SessionId::new();

    let stream = model
        .generate(session, &[Message::user("hi")], CancellationToken::new())
        .await
        .expect("the request is accepted");
    drain(stream).await;

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    let recorded = &requests[0];
    assert_eq!(recorded.path, "/chat/completions", "one endpoint, one path");
    assert_eq!(
        recorded
            .headers
            .get("x-opencode-session")
            .expect("the session header"),
        &session.to_string(),
        "the header is the loop's session id"
    );
    assert_eq!(
        recorded
            .headers
            .get("authorization")
            .expect("authorization"),
        &format!("Bearer {}", api_key())
    );
    assert!(
        recorded
            .headers
            .get("user-agent")
            .expect("a user agent")
            .to_str()
            .expect("text")
            .starts_with("robi/"),
        "the vendor asks a client to name itself"
    );
    assert_eq!(recorded.body["model"], "glm-5.3");
    assert_eq!(recorded.body["stream"], true);
}

#[tokio::test]
async fn the_tools_array_is_sent_and_is_byte_stable() {
    let fake = Fake::scripted(vec![text_stream("ok")]);
    let base = spawn(fake.clone()).await;

    let registry = Arc::new(ToolRegistry::new());
    registry
        .register(Arc::new(StubTool::new("zeta")) as Arc<dyn Tool>)
        .expect("unique name");
    registry
        .register(Arc::new(StubTool::new("alpha")) as Arc<dyn Tool>)
        .expect("unique name");
    let model = build(base, registry);

    for _ in 0..2 {
        let stream = model
            .generate(
                SessionId::new(),
                &[Message::user("hi")],
                CancellationToken::new(),
            )
            .await
            .expect("the request is accepted");
        drain(stream).await;
    }

    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    let tools = requests[0].body["tools"].as_array().expect("a tools array");
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["function"]["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, vec!["alpha", "zeta"], "sorted, so caching holds");
    assert_eq!(
        requests[0].body["tools"], requests[1].body["tools"],
        "the same tools serialize identically on every request"
    );
}

// ---------------------------------------------------------------------------
// Tool-call ids
// ---------------------------------------------------------------------------

/// A provider turn that calls one tool.
fn tool_call_stream(provider_id: &str) -> Reply {
    Reply::Sse {
        payloads: vec![
            format!(
                r#"{{"choices":[{{"delta":{{"tool_calls":[{{"index":0,"id":"{provider_id}","function":{{"name":"read_file","arguments":""}}}}]}}}}]}}"#
            ),
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\"a.rs\"}"}}]}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#.to_owned(),
            "[DONE]".to_owned(),
        ],
        gap: Duration::ZERO,
        headers_delay: Duration::ZERO,
    }
}

#[tokio::test]
async fn the_providers_tool_call_id_is_stored_then_echoed_on_the_next_request() {
    let fake = Fake::scripted(vec![tool_call_stream("call_abc123"), text_stream("done")]);
    let base = spawn(fake.clone()).await;
    let model = build(base, Arc::new(ToolRegistry::new()));
    let session = SessionId::new();

    // Turn one: the model asks for a tool.
    let stream = model
        .generate(
            session,
            &[Message::user("read a.rs")],
            CancellationToken::new(),
        )
        .await
        .expect("the request is accepted");
    let (_, finished) = drain(stream).await;
    let assistant = finished.expect("a message");
    let call = assistant.tool_calls.first().expect("one tool call").clone();
    assert_eq!(
        call.provider_call_id.as_deref(),
        Some("call_abc123"),
        "the provider's id is kept, not just the local one"
    );

    // Turn two: the tool result goes back, and must carry the provider's id.
    let transcript = vec![
        Message::user("read a.rs"),
        assistant,
        Message::tool_result(call.id, "file contents"),
    ];
    let stream = model
        .generate(session, &transcript, CancellationToken::new())
        .await
        .expect("the request is accepted");
    drain(stream).await;

    let requests = fake.requests();
    let second = &requests[1].body["messages"];
    let assistant_entry = second
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["role"] == "assistant")
        .expect("the assistant message");
    let tool_entry = second
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("the tool result");

    assert_eq!(assistant_entry["tool_calls"][0]["id"], "call_abc123");
    assert_eq!(
        tool_entry["tool_call_id"], "call_abc123",
        "both halves of the pair must use the provider's id"
    );
}

#[tokio::test]
async fn a_provider_that_omits_the_tool_call_id_still_produces_a_consistent_request() {
    let fake = Fake::scripted(vec![
        Reply::Sse {
            payloads: vec![
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"read_file","arguments":"{}"}}]}}]}"#.to_owned(),
                r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#.to_owned(),
                "[DONE]".to_owned(),
            ],
            gap: Duration::ZERO,
            headers_delay: Duration::ZERO,
        },
        text_stream("done"),
    ]);
    let base = spawn(fake.clone()).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let stream = model
        .generate(
            SessionId::new(),
            &[Message::user("go")],
            CancellationToken::new(),
        )
        .await
        .expect("the request is accepted");
    let (_, finished) = drain(stream).await;
    let assistant = finished.expect("a message");
    let call = assistant.tool_calls.first().expect("a call").clone();
    assert_eq!(
        call.provider_call_id, None,
        "there was no id to record, so the fallback applies"
    );

    let transcript = vec![assistant, Message::tool_result(call.id, "contents")];
    let stream = model
        .generate(SessionId::new(), &transcript, CancellationToken::new())
        .await
        .expect("the request is accepted");
    drain(stream).await;

    let requests = fake.requests();
    let messages = requests[1].body["messages"].as_array().expect("messages");
    let wire_id = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .expect("assistant")
        .get("tool_calls")
        .and_then(|calls| calls.get(0))
        .and_then(|call| call.get("id"))
        .and_then(|id| id.as_str())
        .expect("a wire id");
    let result_id = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("tool result")["tool_call_id"]
        .as_str()
        .expect("a result id");

    assert_eq!(
        wire_id, result_id,
        "with no provider id, both halves fall back to the local id and still agree"
    );
    assert_eq!(wire_id, call.id.to_string());
}

// ---------------------------------------------------------------------------
// Retries
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_rate_limit_is_retried_and_then_succeeds() {
    let fake = Fake::scripted(vec![
        Reply::Error {
            status: 429,
            body: r#"{"error":"slow down"}"#.to_owned(),
            headers: vec![("retry-after", "0".to_owned())],
        },
        text_stream("recovered"),
    ]);
    let base = spawn(fake.clone()).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let stream = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .expect("the retry succeeds");
    let (deltas, finished) = drain(stream).await;

    assert_eq!(fake.request_count(), 2, "one retry");
    assert_eq!(finished.expect("a message").content, "recovered");
    assert_eq!(
        deltas
            .iter()
            .filter(|delta| matches!(delta, Delta::Finished(_)))
            .count(),
        1,
        "a retry must not duplicate the message"
    );
}

#[tokio::test]
async fn an_unauthorized_response_is_not_retried() {
    let fake = Fake::scripted(vec![Reply::Error {
        status: 401,
        body: r#"{"error":"bad key"}"#.to_owned(),
        headers: Vec::new(),
    }]);
    let base = spawn(fake.clone()).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let error = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .err()
        .expect("a rejected credential is an error");

    assert_eq!(fake.request_count(), 1, "a bad key will not fix itself");
    let text = error.to_string();
    assert!(text.contains("credential"), "{text}");
}

#[tokio::test]
async fn a_server_error_exhausts_the_attempts_and_reports_the_status() {
    let fake = Fake::scripted(vec![Reply::Error {
        status: 500,
        body: r#"{"error":"upstream is down"}"#.to_owned(),
        headers: Vec::new(),
    }]);
    let base = spawn(fake.clone()).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let error = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .err()
        .expect("the attempts run out");

    assert_eq!(fake.request_count(), 3, "max_attempts is three");
    assert!(error.to_string().contains("500"), "{error}");
}

// ---------------------------------------------------------------------------
// Cancellation, timeouts, and mid-stream failures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cancelling_mid_stream_stops_the_deltas() {
    let fake = Fake::scripted(vec![Reply::Sse {
        payloads: vec![
            r#"{"choices":[{"delta":{"content":"one"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{"content":"two"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{"content":"three"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#.to_owned(),
            "[DONE]".to_owned(),
        ],
        gap: Duration::from_millis(50),
        headers_delay: Duration::ZERO,
    }]);
    let base = spawn(fake).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let cancel = CancellationToken::new();
    let mut stream = model
        .generate(SessionId::new(), &[Message::user("hi")], cancel.clone())
        .await
        .expect("the request is accepted");

    // Read one delta, then cancel.
    assert!(stream.next().await.is_some());
    cancel.cancel();

    let mut saw_finished = false;
    while let Some(delta) = stream.next().await {
        if matches!(delta, Delta::Finished(_)) {
            saw_finished = true;
        }
    }
    assert!(
        !saw_finished,
        "a cancelled turn must not append a message, so no finished delta"
    );
}

#[tokio::test]
async fn a_gap_longer_than_the_chunk_timeout_ends_the_turn() {
    let fake = Fake::scripted(vec![Reply::Sse {
        payloads: vec![
            r#"{"choices":[{"delta":{"content":"first"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{"content":"second"}}]}"#.to_owned(),
        ],
        // Longer than the chunk timeout below, so the gap trips it.
        gap: Duration::from_millis(400),
        headers_delay: Duration::ZERO,
    }]);
    let base = spawn(fake).await;

    let mut settings = settings(base);
    settings.chunk_timeout = Duration::from_millis(50);
    let model = build_model(settings, Arc::new(ToolRegistry::new())).expect("builds");

    let stream = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .expect("the request is accepted");
    let (deltas, finished) = drain(stream).await;

    assert!(
        deltas.iter().any(|delta| matches!(delta, Delta::Failed(_))),
        "a stalled stream is reported, not left hanging"
    );
    assert!(finished.is_none(), "an incomplete message is not settled");
}

#[tokio::test]
async fn a_gap_within_the_chunk_timeout_streams_normally() {
    let fake = Fake::scripted(vec![Reply::Sse {
        payloads: vec![
            r#"{"choices":[{"delta":{"content":"first"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{"content":" second"}}]}"#.to_owned(),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#.to_owned(),
            "[DONE]".to_owned(),
        ],
        gap: Duration::from_millis(20),
        headers_delay: Duration::ZERO,
    }]);
    let base = spawn(fake).await;

    let mut settings = settings(base);
    settings.chunk_timeout = Duration::from_secs(5);
    let model = build_model(settings, Arc::new(ToolRegistry::new())).expect("builds");

    let stream = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .expect("the request is accepted");
    let (_, finished) = drain(stream).await;
    assert_eq!(finished.expect("a message").content, "first second");
}

#[tokio::test]
async fn a_failure_after_the_first_delta_does_not_retry() {
    // D7's guarantee: the retry window closes once a delta has reached the loop,
    // because a retry would re-request a message the user is already watching
    // stream. Assert the request count, not just the outcome.
    let fake = Fake::scripted(vec![
        Reply::Sse {
            payloads: vec![
                r#"{"choices":[{"delta":{"content":"partial"}}]}"#.to_owned(),
                r#"{"error":{"message":"upstream died","type":"server_error"}}"#.to_owned(),
            ],
            gap: Duration::ZERO,
            headers_delay: Duration::ZERO,
        },
        text_stream("second attempt"),
    ]);
    let base = spawn(fake.clone()).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let stream = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .expect("the request is accepted");
    let (deltas, finished) = drain(stream).await;

    assert!(
        deltas.iter().any(|delta| matches!(delta, Delta::Failed(_))),
        "the mid-stream error is reported"
    );
    assert!(finished.is_none());
    assert_eq!(
        fake.request_count(),
        1,
        "a mid-stream failure must not re-request a message already streaming"
    );
}

#[tokio::test]
async fn an_error_object_inside_a_200_stream_becomes_a_failure() {
    let fake = Fake::scripted(vec![Reply::Sse {
        payloads: vec![
            r#"{"choices":[{"delta":{"content":"partial"}}]}"#.to_owned(),
            r#"{"error":{"message":"upstream died","type":"server_error"}}"#.to_owned(),
        ],
        gap: Duration::ZERO,
        headers_delay: Duration::ZERO,
    }]);
    let base = spawn(fake).await;
    let model = build(base, Arc::new(ToolRegistry::new()));

    let stream = model
        .generate(
            SessionId::new(),
            &[Message::user("hi")],
            CancellationToken::new(),
        )
        .await
        .expect("the request is accepted");
    let (deltas, finished) = drain(stream).await;

    let failed = deltas.iter().any(|delta| match delta {
        Delta::Failed(error) => error.to_string().contains("upstream died"),
        _ => false,
    });
    assert!(failed, "the error object is reported: {deltas:?}");
    assert!(finished.is_none());
}

// ---------------------------------------------------------------------------
// The loop, driving a real model
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_agent_turn_drives_the_real_adapter_end_to_end() {
    let fake = Fake::scripted(vec![text_stream("the file defines a struct")]);
    let base = spawn(fake.clone()).await;

    let model = build(base, Arc::new(ToolRegistry::new()));
    let store = Arc::new(MemoryStore::default());
    let agent = Agent::new(
        store.clone(),
        Arc::new(NopSink),
        model,
        Arc::new(ToolRegistry::new()),
        LoopConfig::default(),
    );
    let session = agent.new_chat(WorkspaceId::new());

    let outcome = agent
        .user_input(session, "what is in a.rs?", CancellationToken::new())
        .await;

    assert_eq!(outcome, TurnOutcome::Complete);
    let transcript = store.messages(session).await.expect("the transcript loads");
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].role, Role::User);
    assert_eq!(transcript[1].role, Role::Assistant);
    assert_eq!(transcript[1].content, "the file defines a struct");
    assert_eq!(
        fake.request_count(),
        1,
        "one turn with no tools is one model call"
    );
}

#[tokio::test]
async fn a_cancel_while_the_request_is_in_flight_reports_cancelled() {
    // The response headers are delayed past the cancel, so the turn is stopped
    // while `generate` is still awaiting a response. The loop must report
    // `Cancelled` rather than a failure, and must append nothing.
    let fake = Fake::scripted(vec![Reply::Sse {
        payloads: vec![
            r#"{"choices":[{"delta":{"content":"late"}}]}"#.to_owned(),
            "[DONE]".to_owned(),
        ],
        gap: Duration::ZERO,
        headers_delay: Duration::from_millis(500),
    }]);
    let base = spawn(fake.clone()).await;

    let model = build(base, Arc::new(ToolRegistry::new()));
    let store = Arc::new(MemoryStore::default());
    let agent = Arc::new(Agent::new(
        store.clone(),
        Arc::new(NopSink),
        model,
        Arc::new(ToolRegistry::new()),
        LoopConfig::default(),
    ));
    let session = agent.new_chat(WorkspaceId::new());

    let cancel = CancellationToken::new();
    let turn = {
        let agent = agent.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { agent.user_input(session, "hi", cancel).await })
    };

    // Well before the headers arrive, so nothing has been emitted yet.
    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();

    let outcome = tokio::time::timeout(Duration::from_secs(2), turn)
        .await
        .expect("the turn returns promptly once cancelled")
        .expect("the turn task finishes");
    assert_eq!(
        outcome,
        TurnOutcome::Cancelled,
        "a cancel during connection setup is a cancellation, not a failure"
    );

    let transcript = store.messages(session).await.expect("the transcript loads");
    assert!(
        transcript
            .iter()
            .all(|message| message.role != Role::Assistant),
        "nothing is appended for a turn that never produced a message"
    );
}

#[tokio::test]
async fn a_cancel_after_the_headers_still_reports_cancelled() {
    // The complement of the in-flight case: headers arrive, `generate` returns, and
    // the cancel lands while the caller is waiting for the next chunk.
    let fake = Fake::scripted(vec![Reply::Sse {
        payloads: vec![
            r#"{"choices":[{"delta":{"content":"late"}}]}"#.to_owned(),
            "[DONE]".to_owned(),
        ],
        gap: Duration::from_millis(200),
        headers_delay: Duration::ZERO,
    }]);
    let base = spawn(fake).await;

    let model = build(base, Arc::new(ToolRegistry::new()));
    let store = Arc::new(MemoryStore::default());
    let agent = Arc::new(Agent::new(
        store.clone(),
        Arc::new(NopSink),
        model,
        Arc::new(ToolRegistry::new()),
        LoopConfig::default(),
    ));
    let session = agent.new_chat(WorkspaceId::new());

    let cancel = CancellationToken::new();
    let turn = {
        let agent = agent.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { agent.user_input(session, "hi", cancel).await })
    };

    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();

    let outcome = turn.await.expect("the turn task finishes");
    assert_eq!(outcome, TurnOutcome::Cancelled);

    let transcript = store.messages(session).await.expect("the transcript loads");
    assert!(
        transcript
            .iter()
            .all(|message| message.role != Role::Assistant),
        "a cancelled turn must not leave a partial assistant message: {transcript:?}"
    );
}

// ---------------------------------------------------------------------------
// Doubles
// ---------------------------------------------------------------------------

/// A tool that exists only to be listed in a request.
struct StubTool {
    name: String,
}

impl StubTool {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
        }
    }
}

#[async_trait]
impl Tool for StubTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "a tool that exists to be listed"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &serde_json::Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _cancel: CancellationToken,
    ) -> Result<serde_json::Value, robi_core::error::ToolError> {
        Ok(serde_json::json!({"ok": true}))
    }
}

/// A transcript in memory, so a test can drive the real `Agent`.
///
/// `robi-core`'s own in-memory store is test-only and stays inside that crate, so
/// the integration test carries its own.
#[derive(Default)]
struct MemoryStore {
    sessions: Mutex<HashMap<SessionId, Vec<Message>>>,
}

#[async_trait]
impl MessageStore for MemoryStore {
    fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
        let session = SessionId::new();
        self.sessions
            .lock()
            .expect("the store is not poisoned")
            .insert(session, Vec::new());
        session
    }

    fn has_session(&self, session: SessionId) -> bool {
        self.sessions
            .lock()
            .expect("the store is not poisoned")
            .contains_key(&session)
    }

    async fn messages(&self, session: SessionId) -> Result<Vec<Message>, StoreError> {
        Ok(self
            .sessions
            .lock()
            .expect("the store is not poisoned")
            .get(&session)
            .cloned()
            .unwrap_or_default())
    }

    async fn message(
        &self,
        session: SessionId,
        id: MessageId,
    ) -> Result<Option<Message>, StoreError> {
        Ok(self
            .sessions
            .lock()
            .expect("the store is not poisoned")
            .get(&session)
            .and_then(|messages| messages.iter().find(|message| message.id == id).cloned()))
    }

    async fn append(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        self.sessions
            .lock()
            .expect("the store is not poisoned")
            .entry(session)
            .or_default()
            .push(message);
        Ok(())
    }

    async fn update(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        let mut sessions = self.sessions.lock().expect("the store is not poisoned");
        let messages = sessions.entry(session).or_default();
        match messages
            .iter_mut()
            .find(|existing| existing.id == message.id)
        {
            Some(existing) => *existing = message,
            None => messages.push(message),
        }
        Ok(())
    }
}
