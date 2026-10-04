use axum::extract::FromRequest;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use robi_core::message::{
    ApprovalStatus, ExecutionStatus, ImageAttachment, Message, Role, SubagentMode,
    SubagentStepStatus, ToolCall, Usage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    domain::{chat_message::runtime::SubmitOutcome, error::ServiceError},
    web_api::state::AppState,
};

/// The largest image an ingestion request will accept.
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
/// The most images one message may carry.
const MAX_IMAGES_PER_MESSAGE: usize = 8;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/chat_sessions/{id}/messages",
            get(list_chat_messages).post(submit_instruction),
        )
        .route(
            "/api/v1/chat_sessions/{id}/messages/{message_id}",
            get(get_chat_message),
        )
        .route(
            "/api/v1/chat_sessions/{id}/images/{image_id}",
            get(get_image),
        )
        .route(
            "/api/v1/chat_sessions/{id}/tool_calls/{call_id}",
            axum::routing::post(decide_tool_call),
        )
        .route(
            "/api/v1/chat_sessions/{id}/stop",
            axum::routing::post(stop_agent),
        )
        .route(
            "/api/v1/chat_sessions/{id}/tool_originals/{original_id}",
            get(get_tool_original),
        )
        .layer(DefaultBodyLimit::max(
            MAX_IMAGE_BYTES * MAX_IMAGES_PER_MESSAGE + 1024 * 1024,
        ))
        .with_state(state)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/v1/chat_sessions/{id}/messages",
    params(("id" = String, Path, description = "Chat session id")),
    request_body = SubmitInstruction,
    responses(
        (status = 202, description = "Instruction accepted", body = AcceptedInstruction),
        (status = 409, description = "Chat session is awaiting approval")
    )
)]
pub async fn submit_instruction(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: axum::extract::Request,
) -> Result<Response, ServiceError> {
    let session = parse_session_id(&id)?;
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (instruction, images) = if content_type.starts_with("multipart/form-data") {
        let mut multipart = Multipart::from_request(request, &state)
            .await
            .map_err(|error| {
                ServiceError::BadRequest(format!("failed to read the multipart body: {error}"))
            })?;
        ingest_multipart(&mut multipart, session, &state).await?
    } else {
        let payload: SubmitInstruction = axum::Json::from_request(request, &state)
            .await
            .map(|Json(payload)| payload)
            .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
        (payload.instruction, Vec::new())
    };

    match state
        .chat_message_service
        .submit_instruction(session, &instruction, images)
        .await?
    {
        SubmitOutcome::Accepted => {
            tracing::info!(%session, "instruction accepted");
            Ok((
                StatusCode::ACCEPTED,
                Json(AcceptedInstruction {
                    status: "accepted".into(),
                }),
            )
                .into_response())
        }
        SubmitOutcome::AwaitingApproval => Err(ServiceError::Conflict(
            "chat session is awaiting approval".into(),
        )),
    }
}

const SUPPORTED_MEDIA_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

/// Read the multipart body into an `(instruction, images)` pair.
///
/// Enforces, in order: an 8-image cap, a 10 MiB per-image cap, and a magic-byte
/// media-type check. The bytes are written to the session blob file; only the reference
/// (id + media type) returns. The `instruction` field is optional — an image may
/// be sent with no text.
async fn ingest_multipart(
    multipart: &mut Multipart,
    session: robi_core::ids::SessionId,
    state: &AppState,
) -> Result<(String, Vec<ImageAttachment>), ServiceError> {
    let mut instruction = String::new();
    let mut images = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ServiceError::BadRequest(format!("failed to read the body: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        if name == "instruction" {
            let text = field.text().await.map_err(|error| {
                ServiceError::BadRequest(format!("failed to read the instruction: {error}"))
            })?;
            instruction = text;
            continue;
        }
        if name != "images" {
            // An unknown field; ignore it so a future client addition does not
            // break older ones.
            continue;
        }
        if images.len() >= MAX_IMAGES_PER_MESSAGE {
            return Err(ServiceError::BadRequest(format!(
                "a message can carry at most {MAX_IMAGES_PER_MESSAGE} images"
            )));
        }

        let media_type = match field.content_type() {
            Some(media_type) if SUPPORTED_MEDIA_TYPES.contains(&media_type) => {
                media_type.to_owned()
            }
            Some(other) => {
                return Err(ServiceError::UnsupportedMediaType(format!(
                    "unsupported image media type {other}"
                )))
            }
            None => {
                return Err(ServiceError::UnsupportedMediaType(
                    "image parts must declare a content type".into(),
                ))
            }
        };

        let bytes: Bytes = field.bytes().await.map_err(|error| {
            ServiceError::BadRequest(format!("failed to read an image part: {error}"))
        })?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(ServiceError::PayloadTooLarge(format!(
                "image is {} bytes, over the {MAX_IMAGE_BYTES} limit",
                bytes.len()
            )));
        }
        if !matches_media_type(&media_type, &bytes) {
            return Err(ServiceError::UnsupportedMediaType(format!(
                "the bytes do not match a {media_type} image"
            )));
        }

        let id = Uuid::now_v7().to_string();
        state
            .image_source
            .store(&session.to_string(), &id, &media_type, bytes.to_vec())
            .await?;
        images.push(ImageAttachment { id, media_type });
    }
    Ok((instruction, images))
}

/// Magic-byte media type check. No decode, no `image` crate: the signature is
/// enough to catch a mis-typed part before it reaches the provider.
fn matches_media_type(media_type: &str, bytes: &[u8]) -> bool {
    match media_type {
        "image/png" => {
            bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        }
        "image/jpeg" => {
            bytes.len() >= 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff
        }
        // WebP starts with "RIFF" + a 4-byte size + "WEBP".
        "image/webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        // GIF87a or GIF89a.
        "image/gif" => bytes.len() >= 6 && &bytes[0..4] == b"GIF8" && bytes[5] == b'a',
        _ => false,
    }
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions/{id}/messages",
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 200, description = "Chat messages listed", body = Vec<ChatMessage>)
    )
)]
pub async fn list_chat_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ChatMessage>>, ServiceError> {
    let session = parse_session_id(&id)?;
    let messages = state
        .chat_message_service
        .list_messages(session)
        .await?
        .into_iter()
        .map(ChatMessage::from)
        .collect();
    Ok(Json(messages))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions/{id}/messages/{message_id}",
    params(
        ("id" = String, Path, description = "Chat session id"),
        ("message_id" = String, Path, description = "Chat message id")
    ),
    responses(
        (status = 200, description = "Chat message", body = ChatMessage),
        (status = 404, description = "Chat session or message is missing")
    )
)]
pub async fn get_chat_message(
    State(state): State<AppState>,
    Path((id, message_id)): Path<(String, String)>,
) -> Result<Json<ChatMessage>, ServiceError> {
    let session = parse_session_id(&id)?;
    let message = parse_message_id(&message_id)?;
    let message = state
        .chat_message_service
        .get_message(session, message)
        .await?;
    Ok(Json(ChatMessage::from(message)))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions/{id}/images/{image_id}",
    params(
        ("id" = String, Path, description = "Chat session id"),
        ("image_id" = String, Path, description = "A chat image row id")
    ),
    responses(
        (status = 200, description = "The image bytes", content_type = "image/*"),
        (status = 404, description = "Image is missing")
    )
)]
pub async fn get_image(
    State(state): State<AppState>,
    Path((id, image_id)): Path<(String, String)>,
) -> Result<Response, ServiceError> {
    let session = parse_session_id(&id)?;
    // The session id is not a column on the read, but the row belongs to a
    // session; crossing the streams only matters for correctness of access. The
    // store scopes reads by id alone, and the image id is a uuid no caller can
    // guess, so no session check is needed here.
    let _ = session;
    match state.image_source.image(&image_id).await? {
        Some((media_type, bytes)) => Ok((
            [
                (axum::http::header::CONTENT_TYPE, media_type),
                (
                    axum::http::header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable".to_owned(),
                ),
            ],
            bytes,
        )
            .into_response()),
        None => Err(ServiceError::NotFound(format!("image {image_id}"))),
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ToolOriginal {
    #[serde(rename = "shell")]
    Shell {
        stdout: String,
        stderr: String,
        exit_code: i64,
        truncated: bool,
    },
    #[serde(rename = "mcp")]
    Mcp { text: String, truncated: bool },
}

fn original_body(body: &Value) -> ToolOriginal {
    let truncated = body
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if body.get("kind").and_then(Value::as_str) == Some("mcp") {
        return ToolOriginal::Mcp {
            text: body
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            truncated,
        };
    }
    ToolOriginal::Shell {
        stdout: body
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        stderr: body
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        exit_code: body.get("exit_code").and_then(Value::as_i64).unwrap_or(0),
        truncated,
    }
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions/{id}/tool_originals/{original_id}",
    params(
        ("id" = String, Path, description = "Chat session id"),
        ("original_id" = String, Path, description = "Id from a ROBI_LOG header")
    ),
    responses(
        (status = 200, description = "Capped streams stored for a compressed tool result", body = ToolOriginal),
        (status = 404, description = "Original is missing")
    )
)]
pub async fn get_tool_original(
    State(state): State<AppState>,
    Path((id, original_id)): Path<(String, String)>,
) -> Result<Json<ToolOriginal>, ServiceError> {
    let session = parse_session_id(&id)?;
    match state.originals.lookup(session, &original_id).await {
        Ok(crate::agent::compress::Lookup::One(body)) => Ok(Json(original_body(&body))),
        Ok(crate::agent::compress::Lookup::Ambiguous(_))
        | Ok(crate::agent::compress::Lookup::Missing) => Err(ServiceError::NotFound(original_id)),
        Err(error) => {
            tracing::warn!(%error, "tool original lookup failed");
            Err(ServiceError::Unknown)
        }
    }
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/v1/chat_sessions/{id}/tool_calls/{call_id}",
    params(
        ("id" = String, Path, description = "Chat session id"),
        ("call_id" = String, Path, description = "Tool call id")
    ),
    request_body = DecideToolCall,
    responses(
        (status = 202, description = "Decision accepted", body = AcceptedInstruction),
        (status = 409, description = "Chat session is running")
    )
)]
pub async fn decide_tool_call(
    State(state): State<AppState>,
    Path((id, call_id)): Path<(String, String)>,
    Json(payload): Json<DecideToolCall>,
) -> Result<(StatusCode, Json<AcceptedInstruction>), ServiceError> {
    let session = parse_session_id(&id)?;
    let call = parse_tool_call_id(&call_id)?;
    state
        .chat_message_service
        .decide_tool_call(session, call, &payload.decision, payload.reason)
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedInstruction {
            status: "accepted".into(),
        }),
    ))
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/v1/chat_sessions/{id}/stop",
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 202, description = "The actor has exited", body = StoppedAgent),
        (status = 404, description = "Chat session is missing")
    )
)]
pub async fn stop_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<StoppedAgent>), ServiceError> {
    let session = parse_session_id(&id)?;
    state.chat_message_service.stop(session).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(StoppedAgent {
            status: "stopped".into(),
        }),
    ))
}

fn parse_session_id(value: &str) -> Result<robi_core::ids::SessionId, ServiceError> {
    let id =
        Uuid::parse_str(value).map_err(|_| ServiceError::BadRequest("id must be a UUID".into()))?;
    Ok(robi_core::ids::SessionId::from_uuid(id))
}

fn parse_tool_call_id(value: &str) -> Result<robi_core::ids::ToolCallId, ServiceError> {
    let id = Uuid::parse_str(value)
        .map_err(|_| ServiceError::BadRequest("call_id must be a UUID".into()))?;
    Ok(robi_core::ids::ToolCallId::from_uuid(id))
}

fn parse_message_id(value: &str) -> Result<robi_core::ids::MessageId, ServiceError> {
    let id = Uuid::parse_str(value)
        .map_err(|_| ServiceError::BadRequest("message_id must be a UUID".into()))?;
    Ok(robi_core::ids::MessageId::from_uuid(id))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SubmitInstruction {
    pub instruction: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DecideToolCall {
    /// `approve` runs the call. `reject` refuses it.
    pub decision: String,
    /// Shown to the model when `decision` is `reject`. Omitted uses a default.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptedInstruction {
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StoppedAgent {
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<ChatSkill>,
    /// Images the user attached. `id` is an image in the session blob file; fetch the bytes
    /// via `GET /chat_sessions/{id}/images/{image_id}`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ChatImage>,
    pub tool_calls: Vec<ChatToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Present when the provider reported tokens for this model turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
}

/// One image a user attached, referenced by id. The bytes live in the store.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatImage {
    pub id: String,
    pub media_type: String,
}

/// A skill loaded because the user wrote `@id`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatSkill {
    pub id: String,
    pub description: String,
    pub directory: String,
    pub files: Vec<String>,
    pub body: String,
}

/// Token counts for one model turn. `input` is the prompt size, not a session total.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatUsage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub approval_status: String,
    pub execution_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Child tool calls for a `delegate` run. Absent on every other tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent: Option<ChatSubagent>,
    /// Set when compression stored the capped streams for this call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_id: Option<String>,
}

/// One child tool call shown inside a delegate card.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatSubagentStep {
    pub name: String,
    pub target: String,
    pub status: String,
}

/// The child run attached to a parent `delegate` call.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatSubagent {
    pub mode: String,
    pub description: String,
    /// Unix time in milliseconds when the child started.
    pub started_ms: u64,
    pub steps: Vec<ChatSubagentStep>,
}

impl From<Message> for ChatMessage {
    fn from(message: Message) -> Self {
        Self {
            id: message.id.to_string(),
            role: role_name(message.role).to_owned(),
            content: message.content,
            skills: message
                .skills
                .into_iter()
                .map(|skill| ChatSkill {
                    id: skill.id,
                    description: skill.description,
                    directory: skill.directory,
                    files: skill.files,
                    body: skill.body,
                })
                .collect(),
            images: message
                .images
                .into_iter()
                .map(|image| ChatImage {
                    id: image.id,
                    media_type: image.media_type,
                })
                .collect(),
            tool_calls: message
                .tool_calls
                .into_iter()
                .map(ChatToolCall::from)
                .collect(),
            tool_call_id: message.tool_call_id.map(|id| id.to_string()),
            usage: message.usage.map(ChatUsage::from),
        }
    }
}

impl From<Usage> for ChatUsage {
    fn from(usage: Usage) -> Self {
        Self {
            input: usage.input,
            output: usage.output,
            cached: usage.cached,
        }
    }
}

impl From<ToolCall> for ChatToolCall {
    fn from(call: ToolCall) -> Self {
        Self {
            id: call.id.to_string(),
            name: call.name,
            args: call.args,
            approval_status: approval_name(call.approval_status).to_owned(),
            execution_status: execution_name(call.execution_status).to_owned(),
            result: call.result,
            error: call.error,
            original_id: call.original_id,
            subagent: call.subagent.map(|snapshot| ChatSubagent {
                mode: match snapshot.mode {
                    SubagentMode::Explore => "explore",
                    SubagentMode::General => "general",
                }
                .to_owned(),
                description: snapshot.description,
                started_ms: snapshot.started_ms,
                steps: snapshot
                    .steps
                    .into_iter()
                    .map(|step| ChatSubagentStep {
                        name: step.name,
                        target: step.target,
                        status: match step.status {
                            SubagentStepStatus::Running => "running",
                            SubagentStepStatus::Ok => "ok",
                            SubagentStepStatus::Denied => "denied",
                            SubagentStepStatus::Failed => "failed",
                        }
                        .to_owned(),
                    })
                    .collect(),
            }),
        }
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn approval_name(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
    }
}

fn execution_name(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::NotStarted => "not_started",
        ExecutionStatus::Running => "running",
        ExecutionStatus::Succeeded => "succeeded",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
        ExecutionStatus::TimedOut => "timed_out",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robi_core::message::Usage;

    #[test]
    fn a_message_keeps_its_usage() {
        let message = Message::assistant("done").with_usage(Usage {
            input: 10,
            output: 2,
            cached: 4,
        });
        let value = serde_json::to_value(ChatMessage::from(message)).expect("json");
        assert_eq!(value["usage"]["input"], 10);
        assert_eq!(value["usage"]["output"], 2);
        assert_eq!(value["usage"]["cached"], 4);
    }

    #[test]
    fn a_message_without_usage_omits_the_field() {
        let value = serde_json::to_value(ChatMessage::from(Message::user("hi"))).expect("json");
        assert!(value.get("usage").is_none());
    }
}
