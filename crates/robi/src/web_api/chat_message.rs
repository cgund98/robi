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
    ApprovalStatus, ExecutionStatus, FileAttachment, ImageAttachment, Message, Role, SubagentMode,
    SubagentStepStatus, ToolCall, Usage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
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
/// The most file attachments one message may carry.
const MAX_FILES_PER_MESSAGE: usize = 8;
/// The largest text slice one attachment may carry. Well under `read_file`'s
/// 32 KiB window, because an attachment is inlined into every later request.
const MAX_FILE_BYTES: usize = 64 * 1024;
/// The sum of every attachment's text on one message.
const MAX_FILES_TOTAL_BYTES: usize = 256 * 1024;

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
            "/api/v1/chat_sessions/{id}/compact",
            axum::routing::post(compact_session),
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

    let (instruction, images, files) = if content_type.starts_with("multipart/form-data") {
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
        let files = build_files(workspace_root(&state, session).await, payload.files)?;
        (payload.instruction, Vec::new(), files)
    };

    match state
        .chat_message_service
        .submit_instruction(session, &instruction, images, files)
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
) -> Result<(String, Vec<ImageAttachment>, Vec<FileAttachment>), ServiceError> {
    let mut instruction = String::new();
    let mut images = Vec::new();
    let mut files = Vec::new();
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
        if name == "files" {
            // The attachments arrive as one JSON array, since a multipart field
            // cannot carry a nested object on its own. The same caps the JSON
            // route applies run here in `build_files`.
            let text = field.text().await.map_err(|error| {
                ServiceError::BadRequest(format!("failed to read the files part: {error}"))
            })?;
            let inputs: Vec<FileAttachmentInput> =
                serde_json::from_str(&text).map_err(|error| {
                    ServiceError::BadRequest(format!("files must be a JSON array: {error}"))
                })?;
            files = build_files(workspace_root(state, session).await, inputs)?;
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
    Ok((instruction, images, files))
}

/// Validate the client's attachments and turn them into the transcript type.
///
/// The client sends each file's bytes base64-encoded; this decodes them and is
/// the authority on whether the file is text — the name is not consulted. Order:
/// an 8-file count cap, an oversized-payload guard, the per-file cap, the total
/// cap, then the content check. A binary file is `415`, an oversized one `413`,
/// and the rest `400`.
fn build_files(
    root: Option<PathBuf>,
    inputs: Vec<FileAttachmentInput>,
) -> Result<Vec<FileAttachment>, ServiceError> {
    let root = root.as_deref();
    if inputs.len() > MAX_FILES_PER_MESSAGE {
        return Err(ServiceError::BadRequest(format!(
            "a message can carry at most {MAX_FILES_PER_MESSAGE} files"
        )));
    }
    let mut total = 0usize;
    let mut files = Vec::with_capacity(inputs.len());
    for input in inputs {
        if input.name.trim().is_empty() {
            return Err(ServiceError::BadRequest(
                "a file attachment needs a name".into(),
            ));
        }
        // Guard before decoding: base64 is 4/3 of the bytes, so a payload over
        // twice the cap cannot decode to something within it.
        if input.content_base64.len() > MAX_FILE_BYTES * 2 {
            return Err(ServiceError::PayloadTooLarge(format!(
                "{} is over the {MAX_FILE_BYTES} byte limit",
                input.name
            )));
        }
        let bytes = decode_base64(&input.name, &input.content_base64)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(ServiceError::PayloadTooLarge(format!(
                "{} is {} bytes, over the {MAX_FILE_BYTES} limit",
                input.name,
                bytes.len()
            )));
        }
        total += bytes.len();
        if total > MAX_FILES_TOTAL_BYTES {
            return Err(ServiceError::BadRequest(format!(
                "the attached files total more than {MAX_FILES_TOTAL_BYTES} bytes"
            )));
        }
        let text = decode_text(&input.name, bytes)?;
        files.push(FileAttachment {
            name: input.name,
            path: classify_path(input.absolute_path.as_deref(), root),
            start_line: input.start_line,
            end_line: input.end_line,
            text,
        });
    }
    Ok(files)
}

/// The session's workspace root, for deciding in-workspace vs outside. `None`
/// when the session or its workspace cannot be read; every attachment is then
/// treated as outside, which is the safe default.
async fn workspace_root(state: &AppState, session: robi_core::ids::SessionId) -> Option<PathBuf> {
    let chat = state
        .chat_message_service
        .sessions
        .get_chat_session(session)
        .await
        .ok()?;
    let workspace = state
        .chat_message_service
        .sessions
        .workspaces
        .get_workspace(chat.workspace_id)
        .await
        .ok()
        .flatten()?;
    Some(PathBuf::from(workspace.root))
}

/// The workspace-relative path of an attachment, or `None` when it is outside.
///
/// The client sends the file's absolute path when its picker provides one; the
/// server decides here, so the client never classifies. Both paths are
/// canonicalized, which resolves symlinks — a link that points out of the
/// workspace is outside. A path that is not under the root, or cannot be
/// resolved (a file the server cannot see, e.g. a browser upload with no path),
/// is outside and keeps only its name. The server canonicalizes to compare; it
/// never reads the file's contents.
fn classify_path(absolute_path: Option<&str>, root: Option<&std::path::Path>) -> Option<String> {
    let absolute_path = absolute_path?;
    let root = std::fs::canonicalize(root?).ok()?;
    let file = std::fs::canonicalize(absolute_path).ok()?;
    let relative = file.strip_prefix(&root).ok()?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    if relative.is_empty() {
        return None;
    }
    Some(relative)
}

/// Base64-decode one attachment's bytes. A body that is not valid base64 is a
/// malformed request.
fn decode_base64(name: &str, encoded: &str) -> Result<Vec<u8>, ServiceError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| ServiceError::BadRequest(format!("{name} is not valid base64: {error}")))
}

/// The text of one attachment, or `415` when the bytes are not text.
///
/// The same rule the read tools use (`read_code`, `grep`): a NUL byte marks
/// binary, and the bytes must be valid UTF-8. This is the authority — the client
/// sniffs too, for feedback at attach time, but a client that lies is caught
/// here, and no file name or extension is trusted.
fn decode_text(name: &str, bytes: Vec<u8>) -> Result<String, ServiceError> {
    if bytes.contains(&0) {
        return Err(ServiceError::UnsupportedMediaType(format!(
            "{name} is not a text file"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| ServiceError::UnsupportedMediaType(format!("{name} is not utf-8 text")))
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

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/v1/chat_sessions/{id}/compact",
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 202, description = "Compaction accepted", body = CompactingAgent),
        (status = 409, description = "Chat session is running, awaiting approval, or has nothing to compact")
    )
)]
pub async fn compact_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<CompactingAgent>), ServiceError> {
    let session = parse_session_id(&id)?;
    state.chat_message_service.compact(session).await?;
    tracing::info!(%session, "compaction accepted");
    Ok((
        StatusCode::ACCEPTED,
        Json(CompactingAgent {
            status: "compacting".into(),
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
    /// Text files the client read and is attaching. Defaulted so an older client
    /// that sends only `instruction` still parses.
    #[serde(default)]
    pub files: Vec<FileAttachmentInput>,
}

/// One file attachment as the client sends it. The client sends the file's raw
/// bytes base64-encoded; the server decodes them, verifies they are text, and
/// stores the decoded text. The server also decides whether the file is inside
/// the workspace, from `absolute_path` — the client does not classify it.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FileAttachmentInput {
    /// Display name, e.g. `error.rs`.
    pub name: String,
    /// The file's absolute path on the client, when the picker provided one
    /// (the desktop native dialog). Absent for a browser upload, which exposes
    /// only the basename. The server uses it to decide in-workspace vs outside;
    /// it is never stored and never opened.
    #[serde(default)]
    pub absolute_path: Option<String>,
    /// 1-based first line of the slice, when the attach was a range.
    #[serde(default)]
    pub start_line: Option<u32>,
    /// 1-based last line of the slice, inclusive, when the attach was a range.
    #[serde(default)]
    pub end_line: Option<u32>,
    /// The file's bytes, standard base64. The server decodes and checks them.
    pub content_base64: String,
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
pub struct CompactingAgent {
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
    /// Text files the user attached. Metadata only: the attached text stays in
    /// the transcript and is not returned here, so one `GET /messages` does not
    /// carry every attachment's bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ChatFile>,
    pub tool_calls: Vec<ChatToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Present when the provider reported tokens for this model turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
    /// `true` on a compaction summary. Omitted on every other message.
    #[serde(default, skip_serializing_if = "is_false")]
    pub compaction: bool,
}

/// One image a user attached, referenced by id. The bytes live in the store.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatImage {
    pub id: String,
    pub media_type: String,
}

/// One text file a user attached. Metadata for the `filename (1-10)` chip; the
/// attached text is deliberately not returned.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatFile {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
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
            files: message
                .files
                .into_iter()
                .map(|file| ChatFile {
                    name: file.name,
                    path: file.path,
                    start_line: file.start_line,
                    end_line: file.end_line,
                })
                .collect(),
            tool_calls: message
                .tool_calls
                .into_iter()
                .map(ChatToolCall::from)
                .collect(),
            tool_call_id: message.tool_call_id.map(|id| id.to_string()),
            usage: message.usage.map(ChatUsage::from),
            compaction: message.compaction,
        }
    }
}

/// Serde helper: omit `compaction` when it is `false`.
fn is_false(value: &bool) -> bool {
    !*value
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

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::extract::State;
    use robi_core::ids::{SessionId, ToolCallId};
    use uuid::Uuid;

    use crate::domain::{
        chat_message::{runtime::SubmitOutcome, service::ChatMessageService},
        chat_session::service::ChatSessionService,
        error::ServiceError,
        events::EventBus,
        settings::{memory::MemorySettingsStore, store::SettingsStore, SettingsService},
    };
    use crate::web_api::state::AppState;

    /// A runtime that records `compact` calls and returns a scripted result.
    struct RecordingRuntime {
        compacts: AtomicUsize,
        conflict: Option<String>,
    }

    #[async_trait]
    impl crate::domain::chat_message::runtime::ChatRuntime for RecordingRuntime {
        async fn submit(
            &self,
            _session: SessionId,
            _instruction: String,
            _images: Vec<robi_core::message::ImageAttachment>,
            _files: Vec<robi_core::message::FileAttachment>,
        ) -> Result<SubmitOutcome, ServiceError> {
            Ok(SubmitOutcome::Accepted)
        }

        async fn running_session_ids(&self) -> Vec<SessionId> {
            Vec::new()
        }

        async fn decide(
            &self,
            _session: SessionId,
            _call: ToolCallId,
            _reject: Option<String>,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn stop(&self, _session: SessionId) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn compact(&self, _session: SessionId) -> Result<(), ServiceError> {
            self.compacts.fetch_add(1, Ordering::SeqCst);
            match &self.conflict {
                Some(message) => Err(ServiceError::Conflict(message.clone())),
                None => Ok(()),
            }
        }
    }

    async fn state_with(
        runtime: Arc<dyn crate::domain::chat_message::runtime::ChatRuntime>,
    ) -> AppState {
        use crate::adapters::{
            chat_message::SqliteMessageStore, chat_session::repo::SqliteChatSessionRepository,
            sqlite, workspace::repo::SqliteWorkspaceRepository,
        };
        let url = format!(
            "sqlite://file:robi-compact-{}?mode=memory&cache=shared",
            Uuid::now_v7().simple()
        );
        let pool = Arc::new(sqlite::init_pool(&url).await.expect("pool"));
        let workspaces = Arc::new(SqliteWorkspaceRepository::new(Arc::clone(&pool)));
        let workspace_service = Arc::new(crate::domain::workspace::service::WorkspaceService {
            repository: workspaces.clone(),
            asset_cleaner: None,
        });
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(SqliteChatSessionRepository::new(Arc::clone(&pool))),
            workspaces,
            events: None,
            plan_cleaner: None,
        });
        let store: Arc<dyn robi_core::store::MessageStore> = Arc::new(SqliteMessageStore::new(
            Arc::clone(&pool),
            crate::adapters::session_blobs::SessionBlobs::new(
                std::env::temp_dir().join(format!("robi-compact-{}", Uuid::now_v7().simple())),
            ),
        ));
        AppState {
            workspace_service,
            chat_session_service: Arc::clone(&sessions),
            chat_message_service: Arc::new(ChatMessageService {
                sessions,
                runtime,
                store,
            }),
            settings_service: Arc::new(SettingsService {
                store: Arc::new(MemorySettingsStore::new()) as Arc<dyn SettingsStore>,
            }),
            event_bus: Arc::new(EventBus::new()),
            file_changes: Arc::new(
                crate::adapters::file_change::repo::SqliteFileChangeRepository::new(Arc::clone(
                    &pool,
                )),
            ),
            index: Arc::new(crate::agent::index::IndexHub::new(
                std::env::temp_dir(),
                Arc::new(EventBus::new()),
                Arc::new(robi_index::FakeEmbedder::new(4)),
            )),
            mcp: None,
            originals: Arc::new(crate::agent::compress::MemoryOriginals::default()),
            image_source: Arc::new(crate::adapters::chat_image_store::MemoryImageStore::new()),
        }
    }

    #[tokio::test]
    async fn compact_rejects_a_bad_id() {
        let runtime = Arc::new(RecordingRuntime {
            compacts: AtomicUsize::new(0),
            conflict: None,
        });
        let state = state_with(runtime.clone()).await;
        let error = compact_session(State(state), Path("not-a-uuid".into()))
            .await
            .unwrap_err();
        assert_eq!(error, ServiceError::BadRequest("id must be a UUID".into()));
        assert_eq!(runtime.compacts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn compact_maps_a_conflict_to_409() {
        let runtime = Arc::new(RecordingRuntime {
            compacts: AtomicUsize::new(0),
            conflict: Some("nothing to compact".into()),
        });
        let state = state_with(runtime.clone()).await;
        // A workspace and session so the session lookup succeeds.
        let root = std::env::temp_dir().join(format!("robi-compact-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let workspace = state
            .workspace_service
            .open_workspace(root.to_str().unwrap())
            .await
            .unwrap();
        let session = state
            .chat_session_service
            .create_chat_session(
                crate::domain::chat_session::model::CreateChatSessionCommand {
                    workspace_id: workspace.workspace.id,
                    title: None,
                    mode: crate::domain::chat_session::model::AgentMode::Agent,
                    model_config: Default::default(),
                },
            )
            .await
            .unwrap();

        let error = compact_session(State(state), Path(session.id.to_string()))
            .await
            .unwrap_err();
        assert_eq!(error, ServiceError::Conflict("nothing to compact".into()));
        assert_eq!(runtime.compacts.load(Ordering::SeqCst), 1);
    }

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

    #[test]
    fn a_message_carries_file_metadata_but_not_the_text() {
        let message = Message::user("see this").with_files(vec![FileAttachment {
            name: "error.rs".to_owned(),
            path: Some("src/error.rs".to_owned()),
            start_line: Some(29),
            end_line: Some(34),
            text: "the whole slice".to_owned(),
        }]);
        let value = serde_json::to_value(ChatMessage::from(message)).expect("json");
        assert_eq!(
            value["files"],
            serde_json::json!([{
                "name": "error.rs",
                "path": "src/error.rs",
                "start_line": 29,
                "end_line": 34
            }]),
            "the chip metadata is present"
        );
        assert!(
            value["files"][0].get("text").is_none(),
            "the attached text is not returned, so GET /messages stays small"
        );
    }

    #[test]
    fn a_message_without_files_omits_the_field() {
        let value = serde_json::to_value(ChatMessage::from(Message::user("hi"))).expect("json");
        assert!(value.get("files").is_none());
    }

    #[test]
    fn an_upload_keeps_only_the_name() {
        let value = serde_json::to_value(ChatMessage::from(Message::user("").with_files(vec![
            FileAttachment {
                name: "notes.txt".to_owned(),
                path: None,
                start_line: None,
                end_line: None,
                text: "bytes".to_owned(),
            },
        ])))
        .expect("json");
        assert_eq!(value["files"], serde_json::json!([{"name": "notes.txt"}]));
    }

    #[test]
    fn build_files_accepts_a_normal_attachment() {
        let files = build_files(None, vec![input("a.rs", "small")]).expect("under every cap");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "a.rs");
        assert_eq!(files[0].text, "small");
    }

    #[test]
    fn build_files_accepts_a_log_file_because_its_bytes_are_text() {
        // The point of the change: the name and extension are not consulted.
        let files = build_files(None, vec![input("server.log", "2026-01-01 boot\n")])
            .expect("a .log is text by content");
        assert_eq!(files[0].text, "2026-01-01 boot\n");
    }

    #[test]
    fn build_files_rejects_a_binary_file() {
        // A NUL byte marks binary, whatever the name says.
        let error =
            build_files(None, vec![input_bytes("not-really.rs", b"\x00\x01\x02")]).unwrap_err();
        assert!(matches!(error, ServiceError::UnsupportedMediaType(_)));
    }

    #[test]
    fn build_files_rejects_non_utf8_text() {
        let error =
            build_files(None, vec![input_bytes("latin1.txt", b"\xff\xfe\xfa")]).unwrap_err();
        assert!(matches!(error, ServiceError::UnsupportedMediaType(_)));
    }

    #[test]
    fn build_files_rejects_bad_base64() {
        let error = build_files(
            None,
            vec![FileAttachmentInput {
                name: "a.rs".to_owned(),
                absolute_path: None,
                start_line: None,
                end_line: None,
                content_base64: "not base64!!".to_owned(),
            }],
        )
        .unwrap_err();
        assert!(matches!(error, ServiceError::BadRequest(_)));
    }

    #[test]
    fn build_files_rejects_too_many_files() {
        let inputs = (0..MAX_FILES_PER_MESSAGE + 1)
            .map(|index| input(&format!("f{index}.rs"), "x"))
            .collect();
        let error = build_files(None, inputs).unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest(format!(
                "a message can carry at most {MAX_FILES_PER_MESSAGE} files"
            ))
        );
    }

    #[test]
    fn build_files_rejects_an_oversized_file() {
        let big = "x".repeat(MAX_FILE_BYTES + 1);
        let error = build_files(None, vec![input("big.rs", &big)]).unwrap_err();
        assert!(matches!(error, ServiceError::PayloadTooLarge(_)));
    }

    #[test]
    fn build_files_rejects_a_total_over_the_cap() {
        // Each file is under the per-file cap, but together they exceed the total.
        let chunk = "x".repeat(MAX_FILE_BYTES);
        let count = MAX_FILES_TOTAL_BYTES / MAX_FILE_BYTES + 1;
        let inputs = (0..count)
            .map(|index| input(&format!("f{index}.rs"), &chunk))
            .collect();
        let error = build_files(None, inputs).unwrap_err();
        assert!(matches!(error, ServiceError::BadRequest(_)));
    }

    #[test]
    fn build_files_rejects_a_nameless_file() {
        let error = build_files(None, vec![input("   ", "x")]).unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("a file attachment needs a name".into())
        );
    }

    fn base64(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// An outside-workspace attachment: no absolute path.
    fn input(name: &str, text: &str) -> FileAttachmentInput {
        input_bytes(name, text.as_bytes())
    }

    fn input_bytes(name: &str, bytes: &[u8]) -> FileAttachmentInput {
        FileAttachmentInput {
            name: name.to_owned(),
            absolute_path: None,
            start_line: None,
            end_line: None,
            content_base64: base64(bytes),
        }
    }

    /// An attachment from an absolute path; the server classifies it.
    fn at_path(name: &str, absolute_path: &str, text: &str) -> FileAttachmentInput {
        FileAttachmentInput {
            name: name.to_owned(),
            absolute_path: Some(absolute_path.to_owned()),
            start_line: None,
            end_line: None,
            content_base64: base64(text.as_bytes()),
        }
    }

    /// A canonical temp dir with one nested file, as a workspace root with a file.
    fn workspace_with_file(relative: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("robi-attach-{unique}"));
        let file = root.join(relative);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&file, "boom").expect("write");
        (root.canonicalize().expect("canonical root"), file)
    }

    #[test]
    fn an_in_workspace_file_keeps_its_relative_path() {
        let (root, file) = workspace_with_file("src/error.rs");
        let files = build_files(
            Some(root.clone()),
            vec![at_path("error.rs", file.to_str().expect("utf-8"), "boom")],
        )
        .expect("a relative path");
        assert_eq!(files[0].path.as_deref(), Some("src/error.rs"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_outside_the_root_has_no_path() {
        let (root, _) = workspace_with_file("src/error.rs");
        // A real file that is not under the root.
        let outside = root
            .parent()
            .expect("a parent")
            .join("robi-outside-attach.md");
        std::fs::write(&outside, "hi").expect("write");
        let files = build_files(
            Some(root.clone()),
            vec![at_path(
                "robi-outside-attach.md",
                outside.to_str().expect("utf-8"),
                "hi",
            )],
        )
        .expect("it is text");
        assert_eq!(files[0].path, None, "a path outside the root is outside");
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_attachment_with_no_absolute_path_is_outside() {
        // A browser upload, or a client that sent no path.
        let (root, _) = workspace_with_file("src/error.rs");
        let files =
            build_files(Some(root.clone()), vec![input("server.log", "boot")]).expect("it is text");
        assert_eq!(files[0].path, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_root_means_everything_is_outside() {
        let (root, file) = workspace_with_file("src/error.rs");
        let files = build_files(
            None,
            vec![at_path("error.rs", file.to_str().expect("utf-8"), "boom")],
        )
        .expect("it is text");
        assert_eq!(files[0].path, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_symlink_that_leaves_the_root_is_outside() {
        let (root, _) = workspace_with_file("src/error.rs");
        let outside =
            std::env::temp_dir().join(format!("robi-attach-outside-{}", std::process::id()));
        std::fs::write(&outside, "secret").expect("write");
        let link = root.join("leak.md");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");
        let files = build_files(
            Some(root.clone()),
            vec![at_path("leak.md", link.to_str().expect("utf-8"), "hi")],
        )
        .expect("it is text");
        assert_eq!(files[0].path, None, "a symlink out of the root is outside");
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_summary_carries_the_compaction_flag_and_others_omit_it() {
        let summary =
            serde_json::to_value(ChatMessage::from(Message::summary("so far"))).expect("json");
        assert_eq!(summary["compaction"], serde_json::json!(true));

        let plain = serde_json::to_value(ChatMessage::from(Message::user("hi"))).expect("json");
        assert!(
            plain.get("compaction").is_none(),
            "an ordinary message omits the flag"
        );
    }
}
