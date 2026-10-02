use std::collections::HashSet;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use robi_core::ids::{SessionId, WorkspaceId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{
        chat_session::model::{
            ChatSession as DomainChatSession, CreateChatSessionCommand, UpdateChatSessionCommand,
        },
        error::ServiceError,
    },
    web_api::state::AppState,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/chat_sessions",
            get(list_chat_sessions).post(create_chat_session),
        )
        .route(
            "/api/v1/chat_sessions/{id}",
            get(get_chat_session)
                .patch(update_chat_session)
                .delete(delete_chat_session),
        )
        .with_state(state)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/v1/chat_sessions",
    request_body = CreateChatSession,
    responses(
        (status = 201, description = "Chat session created", body = ChatSession)
    )
)]
pub async fn create_chat_session(
    State(state): State<AppState>,
    Json(payload): Json<CreateChatSession>,
) -> Result<(StatusCode, Json<ChatSession>), ServiceError> {
    let workspace_id = parse_workspace_id(&payload.workspace_id)?;
    let session = state
        .chat_session_service
        .create_chat_session(CreateChatSessionCommand {
            workspace_id,
            title: payload.title,
        })
        .await?;
    let running = running_sessions(&state).await;

    Ok((StatusCode::CREATED, Json(to_response(session, &running))))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions",
    params(ListChatSessionsQuery),
    responses(
        (status = 200, description = "Chat sessions listed", body = Vec<ChatSession>)
    )
)]
pub async fn list_chat_sessions(
    State(state): State<AppState>,
    Query(query): Query<ListChatSessionsQuery>,
) -> Result<Json<Vec<ChatSession>>, ServiceError> {
    let workspace_id = match query.workspace_id.as_deref() {
        None => None,
        Some(value) => Some(parse_workspace_id(value)?),
    };
    let sessions = state
        .chat_session_service
        .list_chat_sessions(workspace_id)
        .await?;
    let running = running_sessions(&state).await;
    let sessions = sessions
        .into_iter()
        .map(|session| to_response(session, &running))
        .collect();

    Ok(Json(sessions))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions/{id}",
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 200, description = "Chat session found", body = ChatSession)
    )
)]
pub async fn get_chat_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ChatSession>, ServiceError> {
    let id = parse_chat_session_id(&id)?;
    let session = state.chat_session_service.get_chat_session(id).await?;
    let running = running_sessions(&state).await;
    Ok(Json(to_response(session, &running)))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/api/v1/chat_sessions/{id}",
    request_body = UpdateChatSession,
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 200, description = "Chat session updated", body = ChatSession)
    )
)]
pub async fn update_chat_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateChatSession>,
) -> Result<Json<ChatSession>, ServiceError> {
    let id = parse_chat_session_id(&id)?;
    let session = state
        .chat_session_service
        .update_chat_session(UpdateChatSessionCommand {
            id,
            title: payload.title,
        })
        .await?;
    let running = running_sessions(&state).await;

    Ok(Json(to_response(session, &running)))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/api/v1/chat_sessions/{id}",
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 204, description = "Chat session deleted")
    )
)]
pub async fn delete_chat_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ServiceError> {
    let id = parse_chat_session_id(&id)?;
    state.chat_session_service.delete_chat_session(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn running_sessions(state: &AppState) -> HashSet<SessionId> {
    state
        .chat_message_service
        .running_session_ids()
        .await
        .into_iter()
        .collect()
}

fn to_response(session: DomainChatSession, running: &HashSet<SessionId>) -> ChatSession {
    ChatSession {
        has_pending_agent: running.contains(&session.id),
        id: session.id.to_string(),
        workspace_id: session.workspace_id.to_string(),
        title: session.title,
        created_at: rfc3339(session.created_at),
        updated_at: rfc3339(session.updated_at),
        last_used_at: rfc3339(session.last_used_at),
    }
}

fn parse_chat_session_id(value: &str) -> Result<SessionId, ServiceError> {
    parse_uuid(value, "id").map(SessionId::from_uuid)
}

fn parse_workspace_id(value: &str) -> Result<WorkspaceId, ServiceError> {
    parse_uuid(value, "workspace_id").map(WorkspaceId::from_uuid)
}

fn parse_uuid(value: &str, name: &str) -> Result<Uuid, ServiceError> {
    Uuid::parse_str(value).map_err(|_| ServiceError::BadRequest(format!("{name} must be a UUID")))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateChatSession {
    pub workspace_id: String,
    /// Omitted, null, or empty leaves the chat session unnamed. The model writes a
    /// title after the first turn. Longer than
    /// [`CHAT_SESSION_TITLE_MAX_CHARS`](crate::domain::chat_session::service::CHAT_SESSION_TITLE_MAX_CHARS)
    /// is rejected.
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateChatSession {
    pub title: String,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListChatSessionsQuery {
    pub workspace_id: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ChatSession {
    pub id: String,
    pub workspace_id: String,
    /// Absent until the model names the chat session after the first turn, or a
    /// create/rename supplies one.
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: String,
    /// True while this session's actor is running. Read from memory, not the database.
    pub has_pending_agent: bool,
}

fn rfc3339(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339()
}
