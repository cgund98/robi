use std::path::{Path as FsPath, PathBuf};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
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
        .route(
            "/api/v1/workspaces/{id}/mcp",
            get(list_mcp_servers).post(focus_mcp),
        )
        .route("/api/v1/workspaces/{id}/mcp/config", get(get_mcp_config))
        .route("/api/v1/workspaces/{id}/path", get(resolve_workspace_path))
        .route("/api/v1/workspaces/{id}/open", post(open_workspace_file))
        .route("/api/v1/workspaces/{id}/file", get(read_workspace_file))
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
    if let Some(mcp) = &state.mcp {
        mcp.close(id).await;
    }
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
    // The scan walks the filesystem, so it goes off the async worker. The filter
    // for `user_invocable` is inside the lift: the terminal's `@` completion asks
    // the same question.
    let skills = crate::agent::blocking::call({
        let root = root.clone();
        let home = home.clone();
        move || crate::agent::skills::user_invocable(home.as_deref(), Some(&root))
    })
    .await
    // A join error is a panic inside the scan. Failing the request is honest;
    // answering with an empty list would say the workspace has no skills.
    .map_err(|_| ServiceError::Unknown)?;
    let skills = skills
        .into_iter()
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
    post,
    path = "/api/v1/workspaces/{id}/mcp",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 204, description = "This workspace's MCP servers are starting; other workspaces are closed")
    )
)]
pub async fn focus_mcp(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let Some(mcp) = &state.mcp else {
        return Ok(StatusCode::NO_CONTENT);
    };
    let root = std::path::PathBuf::from(&workspace.root);
    let root = root.canonicalize().unwrap_or(root);
    let opener = mcp.opener();
    mcp.focus(id, &root, workspace.mcp_project_sha256.as_deref(), &opener)
        .await;
    Ok(StatusCode::NO_CONTENT)
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
    let hash = workspace.mcp_project_sha256.clone();
    let mcp = mcp.clone();
    let files = crate::agent::blocking::call(move || {
        let root = root.canonicalize().unwrap_or(root);
        mcp.config_files(&root, hash.as_deref())
    })
    .await
    .map_err(ServiceError::BadRequest)?;
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

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    /// Workspace-relative path. `..` and a path that resolves outside the root are refused.
    pub path: String,
}

/// The canonical absolute path of a file inside the workspace.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AbsolutePath {
    pub absolute: String,
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/path",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("path" = String, Query, description = "Workspace-relative path. A path that leaves the workspace is 400.")
    ),
    responses(
        (status = 200, description = "Canonical absolute path inside the workspace", body = AbsolutePath)
    )
)]
pub async fn resolve_workspace_path(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PathQuery>,
) -> Result<Json<AbsolutePath>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let root = PathBuf::from(&workspace.root);
    let relative = query.path;
    let absolute = crate::agent::blocking::call(move || absolute_inside(&root, &relative))
        .await
        .map_err(ServiceError::BadRequest)??;
    Ok(Json(AbsolutePath {
        absolute: absolute.display().to_string(),
    }))
}

/// Open one workspace file with the stored `file_open_command`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OpenWorkspaceFile {
    /// Workspace-relative path. `..` and a path that resolves outside the root are refused.
    pub path: String,
    /// 1-based line. Omitted when the click has no line.
    #[serde(default)]
    pub line: Option<u32>,
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{id}/open",
    params(("id" = String, Path, description = "Workspace id")),
    request_body = OpenWorkspaceFile,
    responses(
        (status = 204, description = "The editor was started, or it was still running after 10 seconds"),
        (status = 400, description = "The command failed to start or exited within 10 seconds"),
        (status = 404, description = "file_open_command is not set")
    )
)]
pub async fn open_workspace_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<OpenWorkspaceFile>,
) -> Result<StatusCode, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let setting = state
        .settings_service
        .get(crate::domain::settings::keys::FILE_OPEN_COMMAND)
        .await?;
    let Some(template) = setting.value.filter(|value| !value.trim().is_empty()) else {
        return Err(ServiceError::NotFound(
            crate::domain::settings::keys::FILE_OPEN_COMMAND.into(),
        ));
    };
    let root = PathBuf::from(&workspace.root);
    let relative = body.path;
    let line = body.line;
    crate::agent::blocking::call(move || open_with_command(&root, &relative, line, &template))
        .await
        .map_err(ServiceError::BadRequest)??;
    Ok(StatusCode::NO_CONTENT)
}

/// Resolve `relative`, render the template, and start the program off the async runtime.
fn open_with_command(
    root: &FsPath,
    relative: &str,
    line: Option<u32>,
    template: &str,
) -> Result<(), ServiceError> {
    let absolute = absolute_inside(root, relative)?;
    let argv = crate::domain::settings::open_command::render_open_command(
        template,
        &absolute.display().to_string(),
        line,
    )
    .map_err(ServiceError::BadRequest)?;
    let program = argv[0].clone();
    let args = &argv[1..];
    spawn_opener(&program, args).map_err(ServiceError::BadRequest)
}

/// Start `program` and wait up to 10 seconds.
///
/// A process still running at the deadline is left running. A thread reaps it
/// later so it does not stay a zombie, and drains stderr so a full pipe cannot
/// stall the editor. A non-zero exit inside the window is an error.
fn spawn_opener(program: &str, args: &[String]) -> Result<(), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| io_message(&err, program))?;
    let deadline = Duration::from_secs(crate::domain::settings::open_command::OPEN_TIMEOUT_SECONDS);
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                return Err(exit_message(program, status.code(), &stderr));
            }
            Ok(None) if started.elapsed() >= deadline => {
                std::thread::spawn(move || {
                    if let Some(mut pipe) = child.stderr.take() {
                        let _ = std::io::copy(&mut pipe, &mut std::io::sink());
                    }
                    let _ = child.wait();
                });
                return Ok(());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(io_message(&err, program)),
        }
    }
}

fn io_message(err: &std::io::Error, program: &str) -> String {
    let text = err.to_string();
    let text = text.split(" (os error").next().unwrap_or(&text);
    format!("{text}: {program}")
}

fn exit_message(program: &str, code: Option<i32>, stderr: &str) -> String {
    let code = code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "signal".into());
    let stderr = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    let stderr: String = stderr.chars().take(400).collect();
    if stderr.is_empty() {
        format!("{program} exited {code}")
    } else {
        format!("{program} exited {code}: {stderr}")
    }
}

/// Canonical absolute path of `relative` under `root`.
///
/// A blank path, the workspace root itself, and any path that resolves outside
/// the root are [`ServiceError::BadRequest`].
fn absolute_inside(root: &FsPath, relative: &str) -> Result<PathBuf, ServiceError> {
    let relative = relative.trim();
    if relative.is_empty() {
        return Err(ServiceError::BadRequest(
            "path must name a file in the workspace".into(),
        ));
    }
    let home = crate::agent::workspace::user_home().unwrap_or_else(|_| root.to_path_buf());
    let resolved = crate::agent::workspace::resolve_path(root, relative, &home)
        .map_err(|err| ServiceError::BadRequest(err.to_string()))?;
    if resolved.relative.is_empty()
        || resolved.relative == ".."
        || resolved.relative.starts_with("../")
    {
        return Err(ServiceError::BadRequest(
            "path is outside the workspace".into(),
        ));
    }
    Ok(resolved.absolute)
}

/// UTF-8 text the file viewer can show. Larger files are refused.
const MAX_VIEW_BYTES: u64 = 1024 * 1024;

/// The text of one file inside the workspace.
#[derive(Serialize, utoipa::ToSchema)]
pub struct WorkspaceFile {
    pub path: String,
    pub content: String,
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/file",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("path" = String, Query, description = "Workspace-relative path. A path that leaves the workspace is 400.")
    ),
    responses(
        (status = 200, description = "UTF-8 text of the file", body = WorkspaceFile)
    )
)]
pub async fn read_workspace_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PathQuery>,
) -> Result<Json<WorkspaceFile>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let root = PathBuf::from(&workspace.root);
    let relative = query.path.trim().to_owned();
    let path = relative.clone();
    let content = crate::agent::blocking::call(move || read_inside(&root, &relative))
        .await
        .map_err(ServiceError::BadRequest)??;
    Ok(Json(WorkspaceFile { path, content }))
}

/// Read `relative` when it is a UTF-8 file under `root` no larger than [`MAX_VIEW_BYTES`].
fn read_inside(root: &FsPath, relative: &str) -> Result<String, ServiceError> {
    let absolute = absolute_inside(root, relative)?;
    let meta = std::fs::metadata(&absolute)
        .map_err(|_| ServiceError::NotFound(relative.trim().to_owned()))?;
    if !meta.is_file() {
        return Err(ServiceError::BadRequest("path is not a file".into()));
    }
    if meta.len() > MAX_VIEW_BYTES {
        return Err(ServiceError::BadRequest("file is larger than 1 MiB".into()));
    }
    let bytes =
        std::fs::read(&absolute).map_err(|_| ServiceError::NotFound(relative.trim().to_owned()))?;
    String::from_utf8(bytes).map_err(|_| ServiceError::BadRequest("file is not UTF-8".into()))
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{absolute_inside, read_inside};

    fn workspace() -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("robi-abs-{}-{}", std::process::id(), unique()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src").join("main.rs"), "fn main() {}").unwrap();
        root.canonicalize().unwrap()
    }

    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn a_relative_path_becomes_absolute() {
        let root = workspace();
        let absolute = absolute_inside(&root, "src/main.rs").unwrap();
        assert_eq!(absolute, root.join("src").join("main.rs"));
    }

    #[test]
    fn a_path_outside_the_workspace_is_refused() {
        let root = workspace();
        let err = absolute_inside(&root, "../secret").unwrap_err();
        assert!(err.to_string().contains("outside"));
    }

    #[test]
    fn a_blank_path_is_refused() {
        let root = workspace();
        assert!(absolute_inside(&root, "  ").is_err());
    }

    #[test]
    fn a_file_inside_the_workspace_is_read() {
        let root = workspace();
        let content = read_inside(&root, "src/main.rs").unwrap();
        assert_eq!(content, "fn main() {}");
    }
}
