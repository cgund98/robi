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
        .route("/api/v1/workspaces/{id}/skills", get(list_skills))
        .route("/api/v1/workspaces/{id}/mcp", get(list_mcp_servers))
        .route("/api/v1/workspaces/{id}/mcp/config", get(get_mcp_config))
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
    tracing::info!(
        workspace = %opened.workspace.id,
        root = %opened.workspace.root,
        created = opened.created,
        "workspace opened"
    );
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
    state.index.remove_files(id);
    Ok(StatusCode::NO_CONTENT)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/skills",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "User-invocable skills", body = Vec<SkillEntry>)
    )
)]
pub async fn list_skills(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SkillEntry>>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let root = std::path::PathBuf::from(&workspace.root);
    let root = root.canonicalize().unwrap_or(root);
    let home = crate::adapters::settings::home_dir()
        .ok()
        .and_then(|dir| dir.parent().map(|parent| parent.to_path_buf()));
    let skills = crate::skills::scan(home.as_deref(), Some(&root))
        .into_iter()
        .filter(|skill| skill.user_invocable)
        .map(|skill| {
            let scope = skill.scope_name().to_owned();
            SkillEntry {
                id: skill.id,
                label: skill.label,
                description: skill.description,
                scope,
                model_invocable: skill.model_invocable,
                path: skill.directory.display().to_string(),
            }
        })
        .collect();
    Ok(Json(skills))
}

fn to_response(workspace: DomainWorkspace) -> Workspace {
    Workspace {
        id: workspace.id.to_string(),
        name: workspace.name,
        root: workspace.root,
        created_at: rfc3339(workspace.created_at),
    }
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/mcp",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "Configured MCP servers and connection status", body = Vec<McpServer>)
    )
)]
pub async fn list_mcp_servers(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<McpServer>>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let Some(mcp) = &state.mcp else {
        return Ok(Json(Vec::new()));
    };
    let root = std::path::PathBuf::from(&workspace.root);
    let root = root.canonicalize().unwrap_or(root);
    let rows = mcp
        .status(id, &root, workspace.mcp_project_sha256.as_deref())
        .await;
    Ok(Json(
        rows.into_iter()
            .map(|row| McpServer {
                id: row.id,
                status: row.status,
                title: row.title,
                icon: row.icon,
                tool_count: row.tool_count,
            })
            .collect(),
    ))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/mcp/config",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "MCP JSON files as stored on disk", body = McpConfig)
    )
)]
pub async fn get_mcp_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<McpConfig>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let Some(mcp) = &state.mcp else {
        return Ok(Json(McpConfig {
            user_path: String::new(),
            user_text: None,
            project_path: String::new(),
            project_text: None,
            project_enabled: false,
        }));
    };
    let root = std::path::PathBuf::from(&workspace.root);
    let root = root.canonicalize().unwrap_or(root);
    let files = mcp.config_files(&root, workspace.mcp_project_sha256.as_deref());
    Ok(Json(McpConfig {
        user_path: files.user_path,
        user_text: files.user_text,
        project_path: files.project_path,
        project_text: files.project_text,
        project_enabled: files.project_enabled,
    }))
}

pub(crate) fn parse_workspace_id(value: &str) -> Result<WorkspaceId, ServiceError> {
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

/// The MCP JSON files for one workspace. Text is the file on disk. Secrets are not resolved.
#[derive(Serialize, utoipa::ToSchema)]
pub struct McpConfig {
    pub user_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_text: Option<String>,
    pub project_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_text: Option<String>,
    /// True when the stored project hash matches the file's current bytes.
    pub project_enabled: bool,
}

/// One MCP server this workspace is configured to use. Header and env values are omitted.
#[derive(Serialize, utoipa::ToSchema)]
pub struct McpServer {
    pub id: String,
    /// `disconnected`, `starting`, `connected`, or `failed`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `https` URL or `data:image` URI from the server handshake.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub tool_count: u32,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SkillEntry {
    pub id: String,
    pub label: String,
    pub description: String,
    /// `user` or `project`.
    pub scope: String,
    pub model_invocable: bool,
    pub path: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root: String,
    pub created_at: String,
}
