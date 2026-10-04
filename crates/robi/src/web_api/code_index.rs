//! `GET` and `PUT /api/v1/workspaces/{id}/index`.

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use robi_index::IndexState;
use serde::{Deserialize, Serialize};

use crate::{
    domain::error::ServiceError,
    web_api::{state::AppState, workspace::parse_workspace_id},
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{id}/index",
            get(get_index).put(put_index),
        )
        .with_state(state)
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct IndexStatusBody {
    pub state: String,
    pub files_done: u64,
    pub files_total: u64,
    pub error: Option<String>,
}

impl IndexStatusBody {
    pub(crate) fn from_status(status: robi_index::IndexStatus) -> Self {
        Self {
            state: match status.state {
                IndexState::Downloading => "downloading",
                IndexState::Indexing => "indexing",
                IndexState::Ready => "ready",
                IndexState::Paused => "paused",
                IndexState::Failed => "failed",
            }
            .to_owned(),
            files_done: status.files_done,
            files_total: status.files_total,
            error: status.error,
        }
    }
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/index",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "Index status", body = IndexStatusBody)
    )
)]
pub async fn get_index(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<IndexStatusBody>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    state.workspace_service.get_workspace(id).await?;
    Ok(Json(IndexStatusBody::from_status(state.index.status(id))))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct IndexCommand {
    /// `paused` or `running`.
    pub state: String,
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/api/v1/workspaces/{id}/index",
    params(("id" = String, Path, description = "Workspace id")),
    request_body = IndexCommand,
    responses(
        (status = 200, description = "Index status", body = IndexStatusBody),
        (status = 400, description = "state is not paused or running")
    )
)]
pub async fn put_index(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(command): Json<IndexCommand>,
) -> Result<Json<IndexStatusBody>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    state.workspace_service.get_workspace(id).await?;
    let paused = match command.state.as_str() {
        "paused" => true,
        "running" => false,
        _ => {
            return Err(ServiceError::BadRequest(
                "state must be paused or running".into(),
            ));
        }
    };
    state
        .index
        .set_paused(id, paused)
        .map_err(|err| ServiceError::BadRequest(err.to_string()))?;
    Ok(Json(IndexStatusBody::from_status(state.index.status(id))))
}
