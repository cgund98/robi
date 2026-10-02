use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use robi_core::ids::WorkspaceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{error::ServiceError, workspace::model::Workspace as DomainWorkspace},
    web_api::state::AppState,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route(
            "/api/v1/workspaces/{id}",
            get(get_workspace).delete(delete_workspace),
        )
        .with_state(state)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/v1/workspaces",
    request_body = CreateWorkspace,
    responses(
        (status = 201, description = "Workspace created", body = Workspace),
        (status = 200, description = "Workspace already open for this root", body = Workspace)
    )
)]
pub async fn create_workspace(
    State(state): State<AppState>,
    Json(payload): Json<CreateWorkspace>,
) -> Result<(StatusCode, Json<Workspace>), ServiceError> {
    let opened = state
        .workspace_service
        .open_workspace(&payload.root)
        .await?;
    let status = if opened.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(to_response(opened.workspace))))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces",
    responses(
        (status = 200, description = "Workspaces listed", body = Vec<Workspace>)
    )
)]
pub async fn list_workspaces(
    State(state): State<AppState>,
) -> Result<Json<Vec<Workspace>>, ServiceError> {
    let workspaces = state.workspace_service.list_workspaces().await?;
    Ok(Json(workspaces.into_iter().map(to_response).collect()))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "Workspace", body = Workspace)
    )
)]
pub async fn get_workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Workspace>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    Ok(Json(to_response(workspace)))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/api/v1/workspaces/{id}",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 204, description = "Workspace deleted")
    )
)]
pub async fn delete_workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ServiceError> {
    let id = parse_workspace_id(&id)?;
    state.workspace_service.delete_workspace(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn to_response(workspace: DomainWorkspace) -> Workspace {
    Workspace {
        id: workspace.id.to_string(),
        name: workspace.name,
        root: workspace.root,
        created_at: rfc3339(workspace.created_at),
    }
}

fn parse_workspace_id(value: &str) -> Result<WorkspaceId, ServiceError> {
    Uuid::parse_str(value)
        .map(WorkspaceId::from_uuid)
        .map_err(|_| ServiceError::BadRequest("id must be a UUID".to_string()))
}

fn rfc3339(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateWorkspace {
    /// Absolute or relative directory. Stored as its canonical path.
    pub root: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root: String,
    pub created_at: String,
}
