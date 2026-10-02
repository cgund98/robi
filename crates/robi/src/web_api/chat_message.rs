use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use robi_core::message::{ApprovalStatus, ExecutionStatus, Message, Role, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    domain::{chat_message::runtime::SubmitOutcome, error::ServiceError},
    web_api::state::AppState,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/chat_sessions/{id}/messages",
            get(list_chat_messages).post(submit_instruction),
        )
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
    Json(payload): Json<SubmitInstruction>,
) -> Result<(StatusCode, Json<AcceptedInstruction>), ServiceError> {
    let session = parse_session_id(&id)?;
    match state
        .chat_message_service
        .submit_instruction(session, &payload.instruction)
        .await?
    {
        SubmitOutcome::Accepted => Ok((
            StatusCode::ACCEPTED,
            Json(AcceptedInstruction {
                status: "accepted".into(),
            }),
        )),
        SubmitOutcome::AwaitingApproval => Err(ServiceError::Conflict(
            "chat session is awaiting approval".into(),
        )),
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

fn parse_session_id(value: &str) -> Result<robi_core::ids::SessionId, ServiceError> {
    let id =
        Uuid::parse_str(value).map_err(|_| ServiceError::BadRequest("id must be a UUID".into()))?;
    Ok(robi_core::ids::SessionId::from_uuid(id))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SubmitInstruction {
    pub instruction: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptedInstruction {
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Vec<ChatToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
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
}

impl From<Message> for ChatMessage {
    fn from(message: Message) -> Self {
        Self {
            id: message.id.to_string(),
            role: role_name(message.role).to_owned(),
            content: message.content,
            tool_calls: message
                .tool_calls
                .into_iter()
                .map(ChatToolCall::from)
                .collect(),
            tool_call_id: message.tool_call_id.map(|id| id.to_string()),
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
