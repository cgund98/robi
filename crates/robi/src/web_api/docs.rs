//! `GET` the markdown files of a workspace, `GET` one file's content, and
//! `GET` natural language search over the indexed markdown.
//!
//! The docs viewer is a workspace-scoped tree of markdown pages. The listing is
//! paths only; the content is fetched per selection. A recursive listing walks
//! the scannable tree and includes ignored directories as unfetched nodes. A
//! one-level listing returns the immediate children of one directory, including
//! ignored markdown. Hidden entries stay out, so `node_modules/` and `target/`
//! stay out of the recursive walk without a special case when they are ignored.
//!
//! Search runs against one of two engines, chosen by `engine`. `semantic`
//! uses the workspace index and starts it when nothing else has: the request
//! holds a lease, and the hub keeps the task alive for a linger afterwards.
//! `ripgrep` is a literal scan of markdown and does not start the index. The
//! response names the engine and carries the index status, so a caller can
//! say that a partial index gave partial semantic hits.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use ignore::WalkBuilder;
use robi_index::{ChunkHit, IndexState};
use serde::{Deserialize, Serialize};

use crate::{
    agent::docs::{apply_changes, merge3, version_hash, ChangeRange},
    agent::tools::change::{atomic_write, lock_path, record_baseline},
    agent::workspace::workspace_relative,
    domain::error::ServiceError,
    domain::events::EventEnvelope,
    web_api::{code_index::IndexStatusBody, state::AppState, workspace::parse_workspace_id},
};

/// The listing stops after this many files. A larger docs set still opens: the
/// tree is navigational, and this cap only bounds one response.
const MAX_FILES: usize = 500;

/// One page is rendered whole. A bigger file is cut and marked, so a generated
/// page cannot stall the renderer.
const MAX_DOC_BYTES: usize = 512 * 1024;

/// Search draws from the same fused pool as the `semantic_search` tool: the
/// nearest 40 by vector and the top 40 by FTS, merged by reciprocal rank.
const SEARCH_K: usize = 40;

/// How many hits a search returns when `limit` is absent.
const DEFAULT_SEARCH_LIMIT: usize = 10;

/// The most hits one search returns, matching the tool's ceiling.
const MAX_SEARCH_LIMIT: usize = 20;

/// A snippet stops here, so one response never carries a whole section.
const SNIPPET_BYTES: usize = 500;

/// Marks a snippet that was cut short.
const SNIPPET_MARK: &str = "…";

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/workspaces/{id}/docs", get(list_docs))
        .route("/api/v1/workspaces/{id}/docs/search", get(search_docs))
        .route(
            "/api/v1/workspaces/{id}/docs/{*path}",
            get(get_doc).put(put_doc),
        )
        .with_state(state)
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocsListing {
    /// Workspace-relative directory that was listed. Empty for the workspace root.
    pub path: String,
    /// This response includes the children of `path`.
    pub children_fetched: bool,
    pub entries: Vec<DocEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct DocEntry {
    /// Workspace-relative, `/` separated.
    pub path: String,
    pub kind: DocKind,
    /// A gitignore rule excludes this path. An ignored directory is listed
    /// without its descendants until a one-level listing reads it.
    pub ignored: bool,
    /// Set on directories. `true` when this response includes that directory's
    /// children. Absent on files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_fetched: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub enum DocKind {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "directory")]
    Directory,
}

#[derive(Debug, Deserialize)]
pub struct DocsListQuery {
    /// Workspace-relative directory to list. Absent lists the workspace root.
    #[serde(default)]
    pub path: Option<String>,
    /// `true` (default) walks the whole scannable hierarchy. `false` lists one level.
    #[serde(default)]
    pub recursive: Option<bool>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocContent {
    pub path: String,
    pub content: String,
    /// The content hash of `content`. The editor sends this back as
    /// `base_version` on a save.
    pub version: String,
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/docs",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("path" = Option<String>, Query, description = "Workspace-relative directory to list. Absent lists the workspace root."),
        ("recursive" = Option<bool>, Query, description = "When true (default), walk the whole scannable hierarchy under path. When false, list one level, including ignored markdown and directories.")
    ),
    responses(
        (status = 200, description = "Markdown entries under the directory. Ignored directories are included unfetched when the walk is recursive.", body = DocsListing),
        (status = 404, description = "No such directory, or a path outside the workspace")
    )
)]
pub async fn list_docs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<DocsListQuery>,
) -> Result<Json<DocsListing>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let root = PathBuf::from(&workspace.root);
    let relative = query.path.unwrap_or_default();
    let recursive = query.recursive.unwrap_or(true);
    let listing =
        tokio::task::spawn_blocking(move || list_doc_entries(&root, &relative, recursive))
            .await
            .map_err(|_| ServiceError::Unknown)??;
    Ok(Json(listing))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/docs/{path}",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("path" = String, Path, description = "Workspace-relative path to a markdown file")
    ),
    responses(
        (status = 200, description = "The file's text", body = DocContent),
        (status = 400, description = "Not a markdown file, or not UTF-8"),
        (status = 404, description = "No such file, or a path outside the workspace")
    )
)]
pub async fn get_doc(
    State(state): State<AppState>,
    AxumPath((id, path)): AxumPath<(String, String)>,
) -> Result<Json<DocContent>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let root = PathBuf::from(&workspace.root);
    let requested = path.clone();
    let content = tokio::task::spawn_blocking(move || read_markdown(&root, &requested))
        .await
        .map_err(|_| ServiceError::Unknown)??;
    let version = state.docs_edits.insert(&content);
    Ok(Json(DocContent {
        path,
        content,
        version,
    }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DocWriteRequest {
    /// The open chat session, for the checkpoint. Absent when the docs view
    /// has no session; the save still lands, without a baseline.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The version the change set was built from. Absent on a create.
    #[serde(default)]
    pub base_version: Option<String>,
    /// The whole buffer. Sent on a create and after a 409.
    #[serde(default)]
    pub content: Option<String>,
    /// CodeMirror change ranges against `base_version`. The autosave path.
    #[serde(default)]
    pub changes: Option<Vec<ChangeRange>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocWriteResponse {
    pub path: String,
    /// The reconciled text, which may differ from the buffer when it merged.
    pub content: String,
    /// The content hash to use as the next save's `base_version`.
    pub version: String,
    /// `applied`, `merged`, `created`, or `unchanged`.
    pub outcome: String,
}

/// A `409` body: the change set named a base that is no longer cached. The
/// client re-sends the whole buffer against this `version`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct DocWriteConflict {
    pub content: String,
    pub version: String,
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/api/v1/workspaces/{id}/docs/{path}",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("path" = String, Path, description = "Workspace-relative path to a markdown file")
    ),
    request_body = DocWriteRequest,
    responses(
        (status = 200, description = "The reconciled text and its new version", body = DocWriteResponse),
        (status = 400, description = "Not markdown, not UTF-8, a malformed change set, or neither content nor changes"),
        (status = 404, description = "No such file, or a path outside the workspace"),
        (status = 409, description = "The base version is gone; re-send the whole buffer", body = DocWriteConflict)
    )
)]
pub async fn put_doc(
    State(state): State<AppState>,
    AxumPath((id, path)): AxumPath<(String, String)>,
    Json(body): Json<DocWriteRequest>,
) -> Result<Response, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let root = PathBuf::from(&workspace.root);
    let absolute = {
        let root = root.clone();
        let relative = path.clone();
        crate::agent::blocking::call(move || confine_markdown_write(&root, &relative))
            .await
            .map_err(|_| ServiceError::Unknown)??
    };

    // The lock covers the read and the write, so two saves cannot interleave.
    let _guard = lock_path(&absolute).await;

    let existed = {
        let absolute = absolute.clone();
        crate::agent::blocking::call(move || absolute.is_file())
            .await
            .map_err(|_| ServiceError::Unknown)?
    };
    let disk = if existed {
        let absolute = absolute.clone();
        crate::agent::blocking::call(move || read_doc_bytes(&absolute))
            .await
            .map_err(|_| ServiceError::Unknown)??
    } else {
        String::new()
    };
    let disk_version = version_hash(&disk);

    // The client's target text: the whole buffer, or the change set applied to
    // its named base. A missing base with a delta is a 409.
    let client = match body.content.clone() {
        Some(content) => content,
        None => {
            let changes = body
                .changes
                .as_deref()
                .ok_or_else(|| ServiceError::BadRequest("content or changes is required".into()))?;
            let Some(base_version) = body.base_version.as_deref() else {
                return Err(ServiceError::BadRequest(
                    "base_version is required with changes".into(),
                ));
            };
            let Some(base) = state.docs_edits.get(base_version) else {
                return Ok(conflict_response(&disk, &disk_version));
            };
            apply_changes(&base, changes).map_err(ServiceError::BadRequest)?
        }
    };

    let (written, outcome) = if !existed {
        (client, "created")
    } else if body.base_version.as_deref() == Some(disk_version.as_str()) {
        // The base is what the disk holds: the change set applies as sent.
        (client, "applied")
    } else if let Some(base_version) = body.base_version.as_deref() {
        match state.docs_edits.get(base_version) {
            Some(base) => (merge3(&base, &disk, &client), "merged"),
            None => return Ok(conflict_response(&disk, &disk_version)),
        }
    } else {
        return Ok(conflict_response(&disk, &disk_version));
    };

    if existed && written == disk {
        return Ok(Json(DocWriteResponse {
            path,
            content: written,
            version: disk_version,
            outcome: "unchanged".to_owned(),
        })
        .into_response());
    }

    // Best effort: a save must land even when the view has no open session.
    if let Some(session_id) = body.session_id.as_deref().and_then(parse_session_id) {
        if let Err(err) = record_baseline(
            state.file_changes.as_ref(),
            session_id,
            &path,
            &disk,
            !existed,
        )
        .await
        {
            tracing::warn!(%err, %path, "docs save could not record a checkpoint");
        }
    }

    {
        let absolute = absolute.clone();
        let text = written.clone();
        crate::agent::blocking::call(move || atomic_write(&absolute, &text))
            .await
            .map_err(|_| ServiceError::Unknown)?
            .map_err(|err| {
                tracing::warn!(%err, %path, "docs save could not write the file");
                ServiceError::Unknown
            })?;
    }

    let version = state.docs_edits.insert(&written);
    state.event_bus.publish(EventEnvelope::file_changed(
        &id.to_string(),
        &path,
        "user",
        body.session_id.as_deref(),
        outcome,
    ));

    Ok(Json(DocWriteResponse {
        path,
        content: written,
        version,
        outcome: outcome.to_owned(),
    })
    .into_response())
}

fn conflict_response(content: &str, version: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(DocWriteConflict {
            content: content.to_owned(),
            version: version.to_owned(),
        }),
    )
        .into_response()
}

fn parse_session_id(raw: &str) -> Option<robi_core::ids::SessionId> {
    uuid::Uuid::parse_str(raw)
        .ok()
        .map(robi_core::ids::SessionId::from_uuid)
}

/// The absolute path of a markdown file to write.
///
/// A missing file is a create: the parent directory is canonicalized and
/// checked to be inside `root`, and the file is joined onto it.
fn confine_markdown_write(root: &Path, relative: &str) -> Result<PathBuf, ServiceError> {
    if !is_markdown(Path::new(relative)) {
        return Err(ServiceError::BadRequest(
            "only markdown files can be edited".into(),
        ));
    }
    let joined = root.join(relative);
    if joined.exists() {
        let canonical = joined
            .canonicalize()
            .map_err(|_| ServiceError::NotFound(relative.to_owned()))?;
        if !canonical.is_file() {
            return Err(ServiceError::BadRequest("path is not a file".into()));
        }
        let inside = workspace_relative(root, &canonical);
        if inside.starts_with("..") || inside.is_empty() {
            return Err(ServiceError::NotFound(relative.to_owned()));
        }
        return Ok(canonical);
    }
    let parent = joined
        .parent()
        .ok_or_else(|| ServiceError::NotFound(relative.to_owned()))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| ServiceError::NotFound(relative.to_owned()))?;
    let inside = workspace_relative(root, &canonical_parent);
    if inside.starts_with("..") {
        return Err(ServiceError::NotFound(relative.to_owned()));
    }
    let name = joined
        .file_name()
        .ok_or_else(|| ServiceError::NotFound(relative.to_owned()))?;
    Ok(canonical_parent.join(name))
}

/// The whole text of a file, without the read-side byte cap.
fn read_doc_bytes(path: &Path) -> Result<String, ServiceError> {
    let bytes = std::fs::read(path).map_err(|_| ServiceError::Unknown)?;
    String::from_utf8(bytes).map_err(|_| ServiceError::BadRequest("file is not UTF-8".into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchEngine {
    Semantic,
    Ripgrep,
}

impl SearchEngine {
    fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Ripgrep => "ripgrep",
        }
    }

    fn parse(raw: Option<&str>) -> Result<Self, ServiceError> {
        match raw.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Semantic),
            Some("semantic") => Ok(Self::Semantic),
            Some("ripgrep") => Ok(Self::Ripgrep),
            Some(other) => Err(ServiceError::BadRequest(format!(
                "engine must be semantic or ripgrep, not {other}"
            ))),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct DocSearchQuery {
    /// The natural language question, or a literal string for `ripgrep`. Required.
    pub q: Option<String>,
    pub limit: Option<usize>,
    /// `semantic` (default) or `ripgrep`.
    pub engine: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocSearchResult {
    /// The trimmed query that was run.
    pub query: String,
    /// `semantic` or `ripgrep`.
    pub engine: String,
    /// The index state the hits were taken from. For `semantic`, anything but
    /// `ready` means the corpus was incomplete, so the hits may be too.
    /// `ripgrep` does not start the index and does not treat this as partial.
    pub index: IndexStatusBody,
    /// Ranked markdown hits, best first.
    pub hits: Vec<DocSearchHit>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DocSearchHit {
    /// Workspace-relative, `/` separated.
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    /// The heading chain of the section this chunk belongs to, joined by `.`
    /// (`Install.Overview`). Empty when the page did not parse into sections.
    pub title: String,
    /// The chunk's opening text, cut at 500 bytes on a character boundary.
    pub snippet: String,
    /// How many chunks of this document matched. Higher is better.
    pub score: f64,
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/docs/search",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("q" = String, Query, description = "A natural language question, or a literal string when engine is ripgrep. Required, non-blank."),
        ("limit" = Option<usize>, Query, description = "How many hits to return. Default 10, maximum 20."),
        ("engine" = Option<String>, Query, description = "semantic (default) or ripgrep. Anything else is 400.")
    ),
    responses(
        (status = 200, description = "Ranked markdown hits with the engine and the index state they came from", body = DocSearchResult),
        (status = 400, description = "`q` is missing or blank, `limit` is out of range, or `engine` is unknown"),
        (status = 404, description = "No such workspace")
    )
)]
pub async fn search_docs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<DocSearchQuery>,
) -> Result<Json<DocSearchResult>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let (text, limit, engine) = search_args(query)?;
    if engine == SearchEngine::Ripgrep {
        let root = PathBuf::from(&workspace.root);
        let hits = search_markdown_literal(&root, &text, limit).await?;
        // No lease: a literal scan must not start the embedding task.
        let status = state.index.status(id);
        return Ok(Json(DocSearchResult {
            query: text,
            engine: engine.as_str().to_owned(),
            index: IndexStatusBody::from_status(status),
            hits,
        }));
    }
    // The lease starts the task when nothing else has one, and the hub keeps
    // it warm for a linger after this request releases it.
    let _lease = state.index.acquire(id, PathBuf::from(&workspace.root));
    let Some(index) = state.index.index(id) else {
        return Err(ServiceError::Unknown);
    };
    let status = index.status();
    // Weights are not on disk yet: there is nothing to search and no warmed
    // session to embed the query with. The status alone is the answer.
    if status.state == IndexState::Downloading {
        return Ok(Json(DocSearchResult {
            query: text,
            engine: engine.as_str().to_owned(),
            index: IndexStatusBody::from_status(status),
            hits: Vec::new(),
        }));
    }
    let query_vec = index
        .embed_query(&text)
        .await
        .map_err(|_| ServiceError::Unknown)?;
    let blocking = Arc::clone(&index);
    let fts = text.clone();
    // The task's first status is `indexing`, before the schema exists. A query
    // in that window has nothing to rank; the status is the answer. A failure
    // once the index is ready is still an error.
    let not_ready = status.state != IndexState::Ready;
    let hits = match tokio::task::spawn_blocking(move || {
        blocking.search(&query_vec, &fts, SEARCH_K)
    })
    .await
    {
        Ok(Ok(hits)) => hits,
        Ok(Err(_)) | Err(_) if not_ready => {
            return Ok(Json(DocSearchResult {
                query: text,
                engine: engine.as_str().to_owned(),
                index: IndexStatusBody::from_status(index.status()),
                hits: Vec::new(),
            }));
        }
        Ok(Err(_)) | Err(_) => return Err(ServiceError::Unknown),
    };
    // Re-read: the scan may have advanced while the query ran.
    let status = index.status();
    Ok(Json(DocSearchResult {
        query: text,
        engine: engine.as_str().to_owned(),
        index: IndexStatusBody::from_status(status),
        hits: doc_hits(hits, limit),
    }))
}

/// Literal markdown matches, `rg` when it is on `PATH`, otherwise a walk of
/// the same files the docs listing shows.
async fn search_markdown_literal(
    root: &Path,
    pattern: &str,
    limit: usize,
) -> Result<Vec<DocSearchHit>, ServiceError> {
    if let Some(rg) = find_rg() {
        match ripgrep_markdown(&rg, root, pattern, limit).await {
            Ok(hits) => return Ok(hits),
            Err(reason) => {
                tracing::warn!(%reason, "docs ripgrep failed to launch; scanning markdown");
            }
        }
    }
    let root = root.to_path_buf();
    let pattern = pattern.to_owned();
    tokio::task::spawn_blocking(move || literal_markdown_hits(&root, &pattern, limit))
        .await
        .map_err(|_| ServiceError::Unknown)?
}

fn find_rg() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join("rg");
        candidate.is_file().then_some(candidate)
    })
}

async fn ripgrep_markdown(
    rg: &Path,
    root: &Path,
    pattern: &str,
    limit: usize,
) -> Result<Vec<DocSearchHit>, String> {
    let mut child = tokio::process::Command::new(rg)
        .args([
            "--fixed-strings",
            "--ignore-case",
            "--line-number",
            "--no-heading",
            "--color=never",
            "--glob=*.md",
            "--glob=*.markdown",
            "--json",
            "--max-count=25",
            "--",
            pattern,
            ".",
        ])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| err.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "rg has no stdout".to_owned())?;
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut hits = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let read = tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        if let Some(hit) = rg_match_hit(line.trim_end()) {
            hits.push(hit);
        }
    }
    drop(reader);
    let status = child.wait().await.map_err(|err| err.to_string())?;
    // 0 is matches, 1 is none. Anything else is a failed search.
    if hits.is_empty() && !status.success() && status.code() != Some(1) {
        let code = status.code().unwrap_or(-1);
        return Err(format!("rg exited {code}"));
    }
    Ok(rank_files(hits, limit))
}

fn rg_match_hit(line: &str) -> Option<DocSearchHit> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "match" {
        return None;
    }
    let data = value.get("data")?;
    let path = data.get("path")?.get("text")?.as_str()?;
    let line_no = u32::try_from(data.get("line_number")?.as_u64()?).ok()?;
    let text = data.get("lines")?.get("text")?.as_str()?;
    let text = text.trim_end_matches(['\n', '\r']);
    line_hit(path, line_no, text)
}

/// One matching line. `title` is the file name. [`rank_files`] sets `score`.
fn line_hit(path: &str, line_no: u32, text: &str) -> Option<DocSearchHit> {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches("./").to_owned();
    let title = Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&path)
        .to_owned();
    Some(DocSearchHit {
        path,
        start_line: line_no,
        end_line: line_no,
        title,
        snippet: snippet(text),
        score: 0.0,
    })
}

fn literal_markdown_hits(
    root: &Path,
    pattern: &str,
    limit: usize,
) -> Result<Vec<DocSearchHit>, ServiceError> {
    let pattern_folded = pattern.to_lowercase();
    let mut hits = Vec::new();
    for relative in list_markdown_files(root)? {
        let Ok(content) = read_markdown(root, &relative) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            if !line.to_lowercase().contains(&pattern_folded) {
                continue;
            }
            let Some(line_no) = u32::try_from(index + 1).ok() else {
                continue;
            };
            if let Some(hit) = line_hit(&relative, line_no, line) {
                hits.push(hit);
            }
        }
    }
    Ok(rank_files(hits, limit))
}

/// One row per file, the first matching line, ordered by how many lines matched.
fn rank_files(hits: Vec<DocSearchHit>, limit: usize) -> Vec<DocSearchHit> {
    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut first: std::collections::HashMap<String, DocSearchHit> =
        std::collections::HashMap::new();
    for hit in hits {
        *counts.entry(hit.path.clone()).or_insert(0) += 1;
        first.entry(hit.path.clone()).or_insert(hit);
    }
    let mut ranked: Vec<DocSearchHit> = first
        .into_iter()
        .map(|(path, mut hit)| {
            hit.score = f64::from(counts.get(&path).copied().unwrap_or(0));
            hit
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    ranked.truncate(limit);
    ranked
}

/// Validates the query parameters. The query is returned trimmed; a blank one
/// is rejected, as is a `limit` outside 1 to [`MAX_SEARCH_LIMIT`].
fn search_args(query: DocSearchQuery) -> Result<(String, usize, SearchEngine), ServiceError> {
    let text = query.q.unwrap_or_default();
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(ServiceError::BadRequest("q is required".into()));
    }
    let limit = query.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if limit == 0 || limit > MAX_SEARCH_LIMIT {
        return Err(ServiceError::BadRequest(
            "limit must be from 1 to 20".into(),
        ));
    }
    let engine = SearchEngine::parse(query.engine.as_deref())?;
    Ok((text, limit, engine))
}

/// One row per markdown document, best first, capped at `limit`.
///
/// The filter runs before ranking so a non-doc hit cannot hold a ranked slot.
/// Hits arrive best first and several can name the same document, so they are
/// grouped by path: the document keeps its best-scoring chunk as the
/// representative hit, and its score becomes the number of matching chunks.
/// A document that matches many times therefore outranks one that matches
/// once, and no document appears twice. Equal counts keep the order fusion
/// gave, so relevance still decides the tie.
fn doc_hits(hits: Vec<ChunkHit>, limit: usize) -> Vec<DocSearchHit> {
    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut best: std::collections::HashMap<String, (f64, DocSearchHit)> =
        std::collections::HashMap::new();
    for hit in hits {
        if !is_markdown(Path::new(&hit.path)) {
            continue;
        }
        *counts.entry(hit.path.clone()).or_insert(0) += 1;
        // `or_insert_with` keeps the first chunk seen, which is the best one
        // because fusion returns hits in descending score order.
        best.entry(hit.path.clone()).or_insert_with(|| {
            let fused = hit.score;
            (
                fused,
                DocSearchHit {
                    path: hit.path,
                    start_line: hit.start_line,
                    end_line: hit.end_line,
                    title: hit.symbol,
                    snippet: snippet(&hit.body),
                    score: fused,
                },
            )
        });
    }
    let mut ranked: Vec<(f64, DocSearchHit)> = best
        .into_iter()
        .map(|(path, (fused, mut hit))| {
            hit.score = f64::from(counts.get(&path).copied().unwrap_or(0));
            (fused, hit)
        })
        .collect();
    ranked.sort_by(|(left_fused, left), (right_fused, right)| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right_fused.total_cmp(left_fused))
            .then_with(|| left.path.cmp(&right.path))
    });
    ranked.into_iter().take(limit).map(|(_, hit)| hit).collect()
}

/// The opening of a chunk, cut at [`SNIPPET_BYTES`] on a character boundary
/// and marked when something was dropped.
fn snippet(body: &str) -> String {
    if body.len() <= SNIPPET_BYTES {
        return body.to_owned();
    }
    let mut end = SNIPPET_BYTES - SNIPPET_MARK.len();
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &body[..end], SNIPPET_MARK)
}

/// Markdown files under `root`, workspace-relative and `/` separated, sorted.
///
/// Hidden entries and gitignored paths are skipped. Symlinks are not followed.
/// The result stops at [`MAX_FILES`].
pub fn list_markdown_files(root: &Path) -> Result<Vec<String>, ServiceError> {
    let listing = list_doc_entries(root, "", true)?;
    let mut files = listing
        .entries
        .into_iter()
        .filter(|entry| entry.kind == DocKind::File)
        .map(|entry| entry.path)
        .collect::<Vec<_>>();
    files.truncate(MAX_FILES);
    Ok(files)
}

/// List markdown under `relative`, which is workspace-relative and empty at the root.
///
/// `recursive` walks the scannable tree and adds each ignored directory it did
/// not enter, with `children_fetched: false` and no descendants. One level
/// lists immediate children, including ignored markdown and directories, and
/// marks every child directory unfetched.
pub fn list_doc_entries(
    root: &Path,
    relative: &str,
    recursive: bool,
) -> Result<DocsListing, ServiceError> {
    let root = root
        .canonicalize()
        .map_err(|_| ServiceError::NotFound(relative.to_owned()))?;
    let start = resolve_list_dir(&root, relative)?;
    let listed = workspace_relative(&root, &start);
    if listed.starts_with("..") {
        return Err(ServiceError::NotFound(relative.to_owned()));
    }
    let mut entries = if recursive {
        list_recursive(&root, &start)?
    } else {
        list_one_level(&root, &start)?
    };
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(DocsListing {
        path: listed,
        children_fetched: true,
        entries,
    })
}

fn resolve_list_dir(root: &Path, relative: &str) -> Result<PathBuf, ServiceError> {
    if relative.is_empty() || relative == "." {
        return Ok(root.to_path_buf());
    }
    if relative.contains('\0') {
        return Err(ServiceError::NotFound(relative.to_owned()));
    }
    let joined = root.join(relative);
    let canonical = joined
        .canonicalize()
        .map_err(|_| ServiceError::NotFound(relative.to_owned()))?;
    if !canonical.is_dir() {
        return Err(ServiceError::NotFound(relative.to_owned()));
    }
    let inside = workspace_relative(root, &canonical);
    if inside.starts_with("..") {
        return Err(ServiceError::NotFound(relative.to_owned()));
    }
    Ok(canonical)
}

fn ignore_walk(dir: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(dir);
    builder
        .hidden(true)
        .follow_links(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true);
    builder
}

fn list_recursive(root: &Path, start: &Path) -> Result<Vec<DocEntry>, ServiceError> {
    let mut files = Vec::new();
    let mut dirs: Vec<(String, bool)> = Vec::new();
    let mut seen_dirs = HashSet::new();
    for entry in ignore_walk(start).build() {
        if files.len() >= MAX_FILES {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let file_type = entry.file_type();
        if file_type.is_some_and(|kind| kind.is_dir()) {
            if seen_dirs.insert(path.to_path_buf()) {
                let relative = workspace_relative(root, path);
                if !relative.is_empty() && !relative.starts_with("..") {
                    dirs.push((relative, false));
                }
                dirs.extend(ignored_child_dirs(root, path)?);
            }
            continue;
        }
        if !file_type.is_some_and(|kind| kind.is_file()) || !is_markdown(path) {
            continue;
        }
        let relative = workspace_relative(root, path);
        if relative.starts_with("..") || relative.is_empty() {
            continue;
        }
        files.push(relative);
    }
    let stubs: HashSet<String> = dirs
        .iter()
        .filter(|(_, ignored)| *ignored)
        .map(|(path, _)| path.clone())
        .collect();
    let mut entries: Vec<DocEntry> = files
        .iter()
        .map(|path| DocEntry {
            path: path.clone(),
            kind: DocKind::File,
            ignored: false,
            children_fetched: None,
        })
        .collect();
    for (path, ignored) in dirs {
        if !directory_belongs(&path, &files, &stubs) {
            continue;
        }
        entries.push(DocEntry {
            path,
            kind: DocKind::Directory,
            ignored,
            children_fetched: Some(!ignored),
        });
    }
    Ok(entries)
}

/// A directory stays in the tree when it holds a listed file or an ignored stub.
fn directory_belongs(path: &str, files: &[String], stubs: &HashSet<String>) -> bool {
    let prefix = format!("{path}/");
    stubs.contains(path)
        || files.iter().any(|file| file.starts_with(&prefix))
        || stubs.iter().any(|stub| stub.starts_with(&prefix))
}

fn list_one_level(root: &Path, start: &Path) -> Result<Vec<DocEntry>, ServiceError> {
    let yielded = direct_yielded(start);
    // A walk rooted inside an ignored directory still yields its children.
    // Those children stay ignored because the directory itself is.
    let parent_ignored = is_ignored_dir(root, start);
    let mut entries = Vec::new();
    let mut files = 0usize;
    let read = std::fs::read_dir(start).map_err(|_| ServiceError::Unknown)?;
    for child in read {
        let Ok(child) = child else {
            continue;
        };
        let name = child.file_name();
        if is_hidden_name(&name) {
            continue;
        }
        let path = child.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let relative = workspace_relative(root, &path);
        if relative.starts_with("..") || relative.is_empty() {
            continue;
        }
        let ignored = parent_ignored || !yielded.contains(&name);
        if meta.is_dir() {
            entries.push(DocEntry {
                path: relative,
                kind: DocKind::Directory,
                ignored,
                children_fetched: Some(false),
            });
            continue;
        }
        if meta.is_file() && is_markdown(&path) {
            if files >= MAX_FILES {
                continue;
            }
            files += 1;
            entries.push(DocEntry {
                path: relative,
                kind: DocKind::File,
                ignored,
                children_fetched: None,
            });
        }
    }
    Ok(entries)
}

/// Names a depth-1 ignore walk yields directly under `dir`.
fn direct_yielded(dir: &Path) -> HashSet<OsString> {
    let mut builder = ignore_walk(dir);
    builder.max_depth(Some(1));
    let mut names = HashSet::new();
    for entry in builder.build() {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.depth() == 0 {
            continue;
        }
        names.insert(entry.file_name().to_os_string());
    }
    names
}

/// Ignored real directories directly inside `dir`. Hidden names and symlinks are skipped.
fn ignored_child_dirs(root: &Path, dir: &Path) -> Result<Vec<(String, bool)>, ServiceError> {
    let yielded = direct_yielded(dir);
    let mut stubs = Vec::new();
    let read = std::fs::read_dir(dir).map_err(|_| ServiceError::Unknown)?;
    for child in read {
        let Ok(child) = child else {
            continue;
        };
        let name = child.file_name();
        if is_hidden_name(&name) || yielded.contains(&name) {
            continue;
        }
        let path = child.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        let relative = workspace_relative(root, &path);
        if relative.starts_with("..") || relative.is_empty() {
            continue;
        }
        stubs.push((relative, true));
    }
    Ok(stubs)
}

fn is_hidden_name(name: &OsString) -> bool {
    name.to_str().is_some_and(|text| text.starts_with('.'))
}

/// Whether `dir` is excluded by the ignore rules that apply from `root`.
///
/// A directory is ignored when its own name is skipped from its parent, or
/// when an ancestor is. The workspace root is not ignored.
fn is_ignored_dir(root: &Path, dir: &Path) -> bool {
    if dir == root {
        return false;
    }
    let Some(parent) = dir.parent() else {
        return false;
    };
    if parent != root && !parent.starts_with(root) {
        return false;
    }
    let Some(name) = dir.file_name() else {
        return false;
    };
    if parent != root && is_ignored_dir(root, parent) {
        return true;
    }
    !direct_yielded(parent).contains(name)
}

/// The text of one markdown file under `root`.
///
/// `relative` is workspace-relative. A non-markdown name is
/// [`ServiceError::BadRequest`]; a path that resolves outside `root`, or a
/// missing file, is [`ServiceError::NotFound`]. A file that is not UTF-8 is
/// [`ServiceError::BadRequest`]. The text is capped at [`MAX_DOC_BYTES`].
pub fn read_markdown(root: &Path, relative: &str) -> Result<String, ServiceError> {
    if !is_markdown(Path::new(relative)) {
        return Err(ServiceError::BadRequest(
            "only markdown files can be viewed".into(),
        ));
    }
    let joined = root.join(relative);
    let canonical = joined
        .canonicalize()
        .map_err(|_| ServiceError::NotFound(relative.to_owned()))?;
    if !canonical.is_file() {
        return Err(ServiceError::NotFound(relative.to_owned()));
    }
    let inside = workspace_relative(root, &canonical);
    if inside.starts_with("..") || inside.is_empty() {
        return Err(ServiceError::NotFound(relative.to_owned()));
    }
    let bytes =
        std::fs::read(&canonical).map_err(|_| ServiceError::NotFound(relative.to_owned()))?;
    let truncated = bytes.len() > MAX_DOC_BYTES;
    let cut = &bytes[..bytes.len().min(MAX_DOC_BYTES)];
    let mut content = String::from_utf8(cut.to_vec())
        .map_err(|_| ServiceError::BadRequest("file is not UTF-8".into()))?;
    if truncated {
        content.push_str("\n\n[The tail of this file was cut.]\n");
    }
    Ok(content)
}

fn is_markdown(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use axum::extract::{Path as AxumPath, Query, State};
    use robi_core::error::StoreError;
    use robi_core::ids::{MessageId, SessionId, WorkspaceId};
    use robi_core::message::Message;
    use robi_core::store::MessageStore;
    use robi_index::{ChunkHit, ChunkKind, Embedder, FakeEmbedder, IndexState};
    use tower::ServiceExt;

    use crate::{
        domain::{
            chat_message::{
                runtime::{ChatRuntime, SubmitOutcome},
                service::ChatMessageService,
            },
            chat_session::{
                model::{ChatSession, CreateChatSessionCommand, UpdateChatSessionCommand},
                repo::ChatSessionRepository,
                service::ChatSessionService,
            },
            error::ServiceError,
            events::EventBus,
            settings::{memory::MemorySettingsStore, store::SettingsStore, SettingsService},
            workspace::{model::Workspace, repo::WorkspaceRepository, service::WorkspaceService},
        },
        web_api::state::AppState,
    };

    use super::{
        doc_hits, list_doc_entries, list_markdown_files, literal_markdown_hits, read_markdown,
        search_args, search_docs, snippet, DocKind, DocSearchQuery, DocSearchResult, SearchEngine,
        MAX_SEARCH_LIMIT, SNIPPET_BYTES, SNIPPET_MARK,
    };

    fn unique() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    /// A temp workspace with a git root, so `.gitignore` applies.
    fn workspace() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("robi-docs-{}-{}", std::process::id(), unique()));
        fs::create_dir_all(root.join("docs/nested")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("scratch/nested")).unwrap();
        fs::write(root.join(".gitignore"), "secret.md\ntarget/\nscratch/\n").unwrap();
        fs::write(root.join("README.md"), "# Readme\n").unwrap();
        fs::write(root.join("docs/a.md"), "# A\n\nbody\n").unwrap();
        fs::write(root.join("docs/nested/b.markdown"), "b\n").unwrap();
        fs::write(root.join("docs/a.txt"), "not markdown\n").unwrap();
        fs::write(root.join("secret.md"), "ignored\n").unwrap();
        fs::write(root.join("scratch/note.md"), "# note\n").unwrap();
        fs::write(root.join("scratch/nested/deep.md"), "deep\n").unwrap();
        fs::write(root.join(".hidden/x.md"), "hidden\n").unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn listing_keeps_markdown_and_skips_hidden_and_ignored() {
        let root = workspace();
        let files = list_markdown_files(&root).unwrap();
        assert_eq!(
            files,
            vec![
                "README.md".to_owned(),
                "docs/a.md".to_owned(),
                "docs/nested/b.markdown".to_owned()
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn listing_includes_ignored_directories_unfetched() {
        let root = workspace();
        let listing = list_doc_entries(&root, "", true).unwrap();
        assert!(listing.children_fetched);
        assert_eq!(listing.path, "");
        let scratch = listing
            .entries
            .iter()
            .find(|entry| entry.path == "scratch")
            .unwrap();
        assert_eq!(scratch.kind, DocKind::Directory);
        assert!(scratch.ignored);
        assert_eq!(scratch.children_fetched, Some(false));
        assert!(listing.entries.iter().any(|entry| {
            entry.path == "README.md" && !entry.ignored && entry.kind == DocKind::File
        }));
        assert!(listing.entries.iter().any(|entry| {
            entry.path == "docs"
                && !entry.ignored
                && entry.children_fetched == Some(true)
                && entry.kind == DocKind::Directory
        }));
        assert!(!listing
            .entries
            .iter()
            .any(|entry| entry.path == "scratch/note.md" || entry.path == "secret.md"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn one_level_of_an_ignored_directory_lists_markdown_not_grandchildren() {
        let root = workspace();
        let listing = list_doc_entries(&root, "scratch", false).unwrap();
        assert_eq!(listing.path, "scratch");
        assert!(listing.children_fetched);
        let note = listing
            .entries
            .iter()
            .find(|entry| entry.path == "scratch/note.md")
            .unwrap();
        assert_eq!(note.kind, DocKind::File);
        assert!(note.ignored);
        let nested = listing
            .entries
            .iter()
            .find(|entry| entry.path == "scratch/nested")
            .unwrap();
        assert_eq!(nested.kind, DocKind::Directory);
        assert!(nested.ignored);
        assert_eq!(nested.children_fetched, Some(false));
        assert!(!listing
            .entries
            .iter()
            .any(|entry| entry.path.contains("deep")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_listing_path_outside_the_workspace_is_not_found() {
        let root = workspace();
        assert!(matches!(
            list_doc_entries(&root, "../elsewhere", true),
            Err(ServiceError::NotFound(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reading_returns_the_text() {
        let root = workspace();
        let content = read_markdown(&root, "docs/a.md").unwrap();
        assert_eq!(content, "# A\n\nbody\n");
        assert_eq!(
            read_markdown(&root, "docs/nested/b.markdown").unwrap(),
            "b\n"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_file_is_not_found() {
        let root = workspace();
        assert!(matches!(
            read_markdown(&root, "docs/nope.md"),
            Err(ServiceError::NotFound(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_non_markdown_name_is_bad_request() {
        let root = workspace();
        assert!(matches!(
            read_markdown(&root, "docs/a.txt"),
            Err(ServiceError::BadRequest(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_that_escapes_the_workspace_is_not_found() {
        let root = workspace();
        let outside = root
            .parent()
            .unwrap()
            .join(format!("robi-outside-{}.md", unique()));
        fs::write(&outside, "outside\n").unwrap();
        let name = outside.file_name().unwrap().to_string_lossy().into_owned();
        assert!(matches!(
            read_markdown(&root, &format!("../{name}")),
            Err(ServiceError::NotFound(_))
        ));
        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&root);
    }

    /// One workspace row pointing at a temp tree that holds both a markdown
    /// page and a code file, so the filter has something to drop.
    struct FixedWorkspace {
        id: WorkspaceId,
        root: String,
    }

    impl FixedWorkspace {
        fn row(&self) -> Workspace {
            Workspace {
                id: self.id,
                name: "docs".to_owned(),
                root: self.root.clone(),
                mcp_project_sha256: None,
                created_at: chrono::Utc::now(),
            }
        }
    }

    #[async_trait]
    impl WorkspaceRepository for FixedWorkspace {
        async fn canonicalize_root(&self, root: &str) -> Result<String, ServiceError> {
            Ok(root.to_owned())
        }

        async fn get_workspace(&self, id: WorkspaceId) -> Result<Option<Workspace>, ServiceError> {
            Ok((id == self.id).then(|| self.row()))
        }

        async fn get_workspace_by_root(
            &self,
            _root: &str,
        ) -> Result<Option<Workspace>, ServiceError> {
            Ok(Some(self.row()))
        }

        async fn insert_workspace(
            &self,
            _root: &str,
            _name: &str,
        ) -> Result<Workspace, ServiceError> {
            Ok(self.row())
        }

        async fn list_workspaces(&self) -> Result<Vec<Workspace>, ServiceError> {
            Ok(vec![self.row()])
        }

        async fn delete_workspace(&self, _id: WorkspaceId) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn set_mcp_project_sha256(
            &self,
            _id: WorkspaceId,
            _hash: Option<String>,
        ) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    /// Stubs for the AppState fields this route never touches.
    struct Unused;

    #[async_trait]
    impl ChatSessionRepository for Unused {
        async fn create_chat_session(
            &self,
            _command: CreateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            unreachable!("docs search does not touch chat sessions")
        }

        async fn get_chat_session(
            &self,
            _id: SessionId,
        ) -> Result<Option<ChatSession>, ServiceError> {
            Ok(None)
        }

        async fn list_chat_sessions(
            &self,
            _workspace_id: Option<WorkspaceId>,
        ) -> Result<Vec<ChatSession>, ServiceError> {
            unreachable!("docs search does not touch chat sessions")
        }

        async fn update_chat_session(
            &self,
            _command: UpdateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            unreachable!("docs search does not touch chat sessions")
        }

        async fn set_title_if_unset(
            &self,
            _id: SessionId,
            _title: String,
        ) -> Result<Option<ChatSession>, ServiceError> {
            unreachable!("docs search does not touch chat sessions")
        }

        async fn set_turn_display(
            &self,
            _id: SessionId,
            _display: crate::domain::chat_session::model::TurnDisplay,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn set_plan_path(&self, _id: SessionId, _path: String) -> Result<(), ServiceError> {
            unreachable!("docs search does not touch chat sessions")
        }

        async fn delete_chat_session(&self, _id: SessionId) -> Result<(), ServiceError> {
            unreachable!("docs search does not touch chat sessions")
        }
    }

    #[async_trait]
    impl ChatRuntime for Unused {
        async fn submit(
            &self,
            _session: SessionId,
            _instruction: String,
            _images: Vec<robi_core::message::ImageAttachment>,
            _files: Vec<robi_core::message::FileAttachment>,
        ) -> Result<SubmitOutcome, ServiceError> {
            unreachable!("docs search does not submit")
        }

        async fn running_session_ids(&self) -> Vec<SessionId> {
            unreachable!("docs search does not list sessions")
        }

        async fn decide(
            &self,
            _session: SessionId,
            _call: robi_core::ids::ToolCallId,
            _reject: Option<String>,
        ) -> Result<(), ServiceError> {
            unreachable!("docs search does not decide")
        }

        async fn stop(&self, _session: SessionId) -> Result<(), ServiceError> {
            unreachable!("docs search does not stop")
        }

        async fn compact(&self, _session: SessionId) -> Result<(), ServiceError> {
            unreachable!("docs search does not compact")
        }
    }

    #[async_trait]
    impl MessageStore for Unused {
        fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
            unreachable!("docs search does not create sessions")
        }

        fn has_session(&self, _session: SessionId) -> bool {
            false
        }

        async fn messages(&self, _session: SessionId) -> Result<Vec<Message>, StoreError> {
            unreachable!("docs search does not read messages")
        }

        async fn message(
            &self,
            _session: SessionId,
            _id: MessageId,
        ) -> Result<Option<Message>, StoreError> {
            Ok(None)
        }

        async fn append(&self, _session: SessionId, _message: Message) -> Result<(), StoreError> {
            unreachable!("docs search does not append")
        }

        async fn update(&self, _session: SessionId, _message: Message) -> Result<(), StoreError> {
            unreachable!("docs search does not update")
        }

        async fn replace_prefix(
            &self,
            _session: SessionId,
            _delete: &[MessageId],
            _summary: Message,
        ) -> Result<(), StoreError> {
            unreachable!("docs search does not compact")
        }
    }

    struct Fixture {
        state: AppState,
        workspace_id: WorkspaceId,
        root: PathBuf,
        embedder: Arc<FakeEmbedder>,
    }

    /// A temp workspace with one markdown page and one code file, both naming
    /// `zephyr`, behind an AppState whose index runs on the fake embedder.
    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!("robi-docs-search-{}", unique()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("README.md"),
            "# Handbook\n\nThe zephyr protocol keeps sessions warm.\n\n# Notes\n\nA paused zephyr stops scanning.\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "fn zephyr() {}\n").unwrap();
        let root = root.canonicalize().unwrap();
        let workspace_id = WorkspaceId::new();
        let embedder = Arc::new(FakeEmbedder::new(4));
        let fanout = Arc::new(EventBus::new());
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(Unused),
            workspaces: Arc::new(crate::domain::workspace::repo::AnyWorkspace),
            events: None,
            plan_cleaner: None,
        });
        let state = AppState {
            workspace_service: Arc::new(WorkspaceService {
                repository: Arc::new(FixedWorkspace {
                    id: workspace_id,
                    root: root.to_string_lossy().into_owned(),
                }),
                asset_cleaner: None,
            }),
            chat_session_service: Arc::clone(&sessions),
            chat_message_service: Arc::new(ChatMessageService {
                sessions,
                runtime: Arc::new(Unused),
                store: Arc::new(Unused),
            }),
            settings_service: Arc::new(SettingsService {
                store: Arc::new(MemorySettingsStore::new()) as Arc<dyn SettingsStore>,
            }),
            event_bus: Arc::clone(&fanout),
            file_changes: Arc::new(
                crate::domain::file_change::memory::MemoryFileChangeRepository::new(),
            ),
            index: Arc::new(crate::agent::index::IndexHub::new(
                std::env::temp_dir(),
                fanout,
                Arc::clone(&embedder) as Arc<dyn Embedder>,
            )),
            mcp: None,
            originals: Arc::new(crate::agent::compress::MemoryOriginals::default()),
            image_source: Arc::new(crate::adapters::chat_image_store::MemoryImageStore::new()),
            docs_edits: Arc::new(crate::agent::docs::DocsEditCache::default()),
        };
        Fixture {
            state,
            workspace_id,
            root,
            embedder,
        }
    }

    async fn run_search(
        fixture: &Fixture,
        q: Option<&str>,
        limit: Option<usize>,
    ) -> Result<DocSearchResult, ServiceError> {
        search_docs(
            State(fixture.state.clone()),
            AxumPath(fixture.workspace_id.to_string()),
            Query(DocSearchQuery {
                q: q.map(str::to_owned),
                limit,
                engine: None,
            }),
        )
        .await
        .map(|body| body.0)
    }

    fn hit(path: &str, score: f64) -> ChunkHit {
        ChunkHit {
            id: 1,
            path: path.to_owned(),
            start_line: 1,
            end_line: 9,
            symbol: "Section".to_owned(),
            language: "rust".to_owned(),
            kind: ChunkKind::Symbol,
            body: "body".to_owned(),
            score,
        }
    }

    #[test]
    fn search_args_trims_and_bounds_the_query() {
        let blank = search_args(DocSearchQuery {
            q: Some("   ".to_owned()),
            limit: None,
            engine: None,
        });
        assert!(matches!(blank, Err(ServiceError::BadRequest(_))));
        let missing = search_args(DocSearchQuery {
            q: None,
            limit: None,
            engine: None,
        });
        assert!(matches!(missing, Err(ServiceError::BadRequest(_))));
        let too_many = search_args(DocSearchQuery {
            q: Some("q".to_owned()),
            limit: Some(MAX_SEARCH_LIMIT + 1),
            engine: None,
        });
        assert!(matches!(too_many, Err(ServiceError::BadRequest(_))));
        let zero = search_args(DocSearchQuery {
            q: Some("q".to_owned()),
            limit: Some(0),
            engine: None,
        });
        assert!(matches!(zero, Err(ServiceError::BadRequest(_))));
        let unknown = search_args(DocSearchQuery {
            q: Some("q".to_owned()),
            limit: None,
            engine: Some("nope".to_owned()),
        });
        assert!(matches!(unknown, Err(ServiceError::BadRequest(_))));
        let ok = search_args(DocSearchQuery {
            q: Some("  zephyr  ".to_owned()),
            limit: Some(3),
            engine: None,
        })
        .unwrap();
        assert_eq!(ok, ("zephyr".to_owned(), 3, SearchEngine::Semantic));
        let text = search_args(DocSearchQuery {
            q: Some("zephyr".to_owned()),
            limit: None,
            engine: Some("ripgrep".to_owned()),
        })
        .unwrap();
        assert_eq!(text.2, SearchEngine::Ripgrep);
    }

    #[test]
    fn a_literal_scan_keeps_markdown_lines_and_caps_the_snippet() {
        let root = std::env::temp_dir().join(format!("robi-docs-rg-{}", unique()));
        fs::create_dir_all(root.join("src")).unwrap();
        let long = format!("zephyr {}", "é".repeat(600));
        fs::write(
            root.join("README.md"),
            format!("# Handbook\n{long}\nzephyr again\n"),
        )
        .unwrap();
        fs::write(root.join("docs-note.md"), "zephyr once\n").unwrap();
        fs::write(root.join("src/lib.rs"), "fn zephyr() {}\n").unwrap();
        let hits = literal_markdown_hits(&root, "ZEPHYR", 10).unwrap();
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].path, "README.md");
        assert_eq!(hits[0].start_line, 2);
        assert_eq!(hits[0].end_line, 2);
        assert_eq!(hits[0].title, "README.md");
        assert_eq!(hits[0].score, 2.0);
        assert_eq!(hits[1].path, "docs-note.md");
        assert_eq!(hits[1].score, 1.0);
        assert!(hits[0].snippet.len() <= SNIPPET_BYTES);
        assert!(hits[0].snippet.ends_with(SNIPPET_MARK));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn doc_hits_drops_code_after_fusion_and_then_limits() {
        // A code hit fused first must not take a slot from a later doc.
        let hits = vec![
            hit("src/lib.rs", 9.0),
            hit("docs/a.md", 8.0),
            hit("docs/b.markdown", 7.0),
            hit("README.md", 6.0),
        ];
        let all = doc_hits(hits, MAX_SEARCH_LIMIT);
        let paths: Vec<&str> = all.iter().map(|hit| hit.path.as_str()).collect();
        assert_eq!(paths, ["docs/a.md", "docs/b.markdown", "README.md"]);

        let ranked = doc_hits(
            vec![
                hit("src/lib.rs", 9.0),
                hit("docs/a.md", 8.0),
                hit("README.md", 6.0),
            ],
            2,
        );
        let paths: Vec<&str> = ranked.iter().map(|hit| hit.path.as_str()).collect();
        assert_eq!(paths, ["docs/a.md", "README.md"]);
        assert!(all.iter().all(|hit| hit.title == "Section"));
    }

    #[test]
    fn doc_hits_groups_chunks_by_document_and_ranks_by_matches() {
        // Several chunks of one document must collapse to one row, and the row
        // with the most matching chunks wins.
        let hits = vec![
            hit("docs/a.md", 9.0),
            hit("docs/a.md", 8.0),
            hit("README.md", 7.0),
            hit("docs/b.md", 6.0),
            hit("docs/b.md", 5.0),
            hit("docs/a.md", 4.0),
        ];
        let ranked = doc_hits(hits, MAX_SEARCH_LIMIT);
        let paths: Vec<&str> = ranked.iter().map(|hit| hit.path.as_str()).collect();
        assert_eq!(paths, ["docs/a.md", "docs/b.md", "README.md"]);
        let scores: Vec<f64> = ranked.iter().map(|hit| hit.score).collect();
        assert_eq!(scores, [3.0, 2.0, 1.0]);
    }

    #[test]
    fn a_snippet_stops_at_the_cap_on_a_character_boundary() {
        let short = "opening lines";
        assert_eq!(snippet(short), short);

        let long = "é".repeat(600);
        let cut = snippet(&long);
        assert!(cut.len() <= SNIPPET_BYTES, "{}", cut.len());
        assert!(cut.ends_with(SNIPPET_MARK));
        assert!(cut.trim_end_matches(SNIPPET_MARK).len() < long.len());
    }

    #[tokio::test]
    async fn a_query_starts_the_index_and_only_docs_come_back() {
        let fixture = fixture();
        let first = run_search(&fixture, Some("zephyr"), None).await.unwrap();
        assert_eq!(first.query, "zephyr");
        let index = fixture
            .state
            .index
            .index(fixture.workspace_id)
            .expect("the request leased the index");
        for _ in 0..250 {
            let state = index.status().state;
            if state == IndexState::Ready || state == IndexState::Failed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            index.status().state,
            IndexState::Ready,
            "{:?}",
            index.status()
        );
        let embedded = fixture.embedder.document_calls();

        let result = run_search(&fixture, Some("zephyr"), None).await.unwrap();
        assert_eq!(result.index.state, "ready");
        assert!(
            result.hits.iter().any(|hit| hit.path == "README.md"),
            "expected the doc hit: {:?}",
            result.hits
        );
        assert!(
            result
                .hits
                .iter()
                .all(|hit| hit.path.ends_with(".md") || hit.path.ends_with(".markdown")),
            "a code hit escaped the filter: {:?}",
            result.hits
        );
        assert!(
            result
                .hits
                .iter()
                .any(|hit| hit.title == "Handbook" || hit.title == "Notes"),
            "the section chain is the title: {:?}",
            result.hits
        );
        // Two sections of README.md match `zephyr`; the document must be one
        // hit, not one per section.
        let readme = result
            .hits
            .iter()
            .filter(|hit| hit.path == "README.md")
            .count();
        assert_eq!(
            readme, 1,
            "a document appeared once per section: {:?}",
            result.hits
        );
        let readme_hit = result
            .hits
            .iter()
            .find(|hit| hit.path == "README.md")
            .unwrap();
        assert_eq!(
            readme_hit.score, 2.0,
            "the score counts the matching sections: {:?}",
            result.hits
        );
        let mut paths: Vec<&str> = result.hits.iter().map(|hit| hit.path.as_str()).collect();
        paths.sort_unstable();
        let before = paths.len();
        paths.dedup();
        assert_eq!(before, paths.len(), "duplicate paths: {:?}", result.hits);

        // The lease is gone, but the linger keeps the task, so the next query
        // does not restart the scan.
        assert!(fixture.state.index.index(fixture.workspace_id).is_some());
        assert_eq!(
            fixture.embedder.document_calls(),
            embedded,
            "a second query inside the linger must not rescan"
        );
        let _ = fs::remove_dir_all(&fixture.root);
    }

    #[tokio::test]
    async fn ripgrep_does_not_start_the_index_and_skips_code() {
        let fixture = fixture();
        let result = search_docs(
            State(fixture.state.clone()),
            AxumPath(fixture.workspace_id.to_string()),
            Query(DocSearchQuery {
                q: Some("zephyr".to_owned()),
                limit: None,
                engine: Some("ripgrep".to_owned()),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(result.engine, "ripgrep");
        assert!(
            fixture.state.index.index(fixture.workspace_id).is_none(),
            "a literal search must not lease the index"
        );
        assert!(
            result.hits.iter().any(|hit| hit.path == "README.md"),
            "{:?}",
            result.hits
        );
        assert!(
            result
                .hits
                .iter()
                .all(|hit| hit.path.ends_with(".md") || hit.path.ends_with(".markdown")),
            "{:?}",
            result.hits
        );
        let _ = fs::remove_dir_all(&fixture.root);
    }

    #[tokio::test]
    async fn the_search_route_answers_and_rejects_a_missing_query() {
        use axum::body::{to_bytes, Body};
        use axum::http::StatusCode;

        fn get(uri: &str) -> axum::http::Request<Body> {
            axum::http::Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap()
        }

        let fixture = fixture();
        // Building the router is the route-conflict check: `/docs/search` must
        // win over the `/docs/{*path}` catch-all rather than collide with it.
        let router = crate::web_api::router(fixture.state.clone());

        let base = format!("/api/v1/workspaces/{}/docs/search", fixture.workspace_id);
        let response = router
            .clone()
            .oneshot(get(&format!("{base}?q=zephyr")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value["hits"].is_array(), "{value}");
        assert_eq!(value["engine"], "semantic");
        assert!(value["index"]["state"].is_string(), "{value}");

        let response = router.clone().oneshot(get(&base)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = router
            .clone()
            .oneshot(get(&format!("{base}?q=zephyr&limit=99")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = router
            .oneshot(get(&format!("{base}?q=zephyr&engine=nope")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let _ = fs::remove_dir_all(&fixture.root);
    }

    use super::{put_doc, DocWriteRequest};
    use crate::agent::docs::{version_hash, ChangeRange};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    fn delta(from: usize, to: usize, insert: &str) -> ChangeRange {
        ChangeRange {
            from,
            to,
            insert: insert.to_owned(),
        }
    }

    /// Call the handler directly and read the status and JSON body.
    async fn put(
        fixture: &Fixture,
        path: &str,
        body: DocWriteRequest,
    ) -> (axum::http::StatusCode, serde_json::Value) {
        let outcome = put_doc(
            State(fixture.state.clone()),
            AxumPath((fixture.workspace_id.to_string(), path.to_owned())),
            axum::Json(body),
        )
        .await;
        match outcome {
            Ok(response) => {
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
                    .await
                    .unwrap();
                (status, serde_json::from_slice(&bytes).unwrap())
            }
            Err(err) => (err.into_response().status(), serde_json::Value::Null),
        }
    }

    #[tokio::test]
    async fn put_applies_a_delta_when_the_base_matches_the_disk() {
        let fixture = fixture();
        let path = "README.md";
        let absolute = fixture.root.join(path);
        let disk = fs::read_to_string(&absolute).unwrap();
        let version = version_hash(&disk);
        // A GET seeds the version cache; the delta can then be applied.
        fixture.state.docs_edits.put(version.clone(), &disk);

        let (status, body) = put(
            &fixture,
            path,
            DocWriteRequest {
                session_id: None,
                base_version: Some(version),
                content: None,
                changes: Some(vec![delta(0, 0, "X")]),
            },
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], "applied");
        let written = fs::read_to_string(&absolute).unwrap();
        assert!(written.starts_with('X'), "{written}");
        assert_eq!(body["content"], written);
        assert_ne!(body["version"], serde_json::Value::Null);
        let _ = fs::remove_dir_all(&fixture.root);
    }

    #[tokio::test]
    async fn put_records_one_baseline_across_repeated_autosaves() {
        let fixture = fixture();
        let session = SessionId::new();
        let path = "README.md";
        let absolute = fixture.root.join(path);
        let original = fs::read_to_string(&absolute).unwrap();
        let mut version = version_hash(&original);
        fixture.state.docs_edits.put(version.clone(), &original);

        for _ in 0..3 {
            let (status, body) = put(
                &fixture,
                path,
                DocWriteRequest {
                    session_id: Some(session.to_string()),
                    base_version: Some(version.clone()),
                    content: None,
                    changes: Some(vec![delta(0, 0, "X")]),
                },
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            version = body["version"].as_str().unwrap().to_owned();
        }

        // Every save kept the bytes from before the first one.
        let baseline = fixture
            .state
            .file_changes
            .get_baseline(session, path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(baseline.baseline, original);
        assert!(!baseline.created);
        let _ = fs::remove_dir_all(&fixture.root);
    }

    #[tokio::test]
    async fn put_merges_an_agent_edit_with_a_client_edit() {
        let fixture = fixture();
        let path = "README.md";
        let absolute = fixture.root.join(path);
        let original = fs::read_to_string(&absolute).unwrap();
        let client_base = version_hash(&original);
        // The client fetched this version; the server still has the bytes.
        fixture.state.docs_edits.put(client_base.clone(), &original);
        // An agent rewrote the heading while the client was editing.
        fs::write(&absolute, original.replace("Handbook", "Manual")).unwrap();

        // The client inserted a line after the first line, which the agent's
        // change to that same line does not overlap.
        let insert_at = original.split('\n').next().unwrap().encode_utf16().count() + 1;
        let (status, body) = put(
            &fixture,
            path,
            DocWriteRequest {
                session_id: None,
                base_version: Some(client_base),
                content: None,
                changes: Some(vec![delta(insert_at, insert_at, "X\n")]),
            },
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], "merged");
        let written = fs::read_to_string(&absolute).unwrap();
        assert!(written.contains("Manual"), "{written}");
        assert!(written.contains("\nX\n"), "{written}");
        let _ = fs::remove_dir_all(&fixture.root);
    }

    #[tokio::test]
    async fn put_conflicts_when_the_base_version_is_gone() {
        let fixture = fixture();
        let path = "README.md";
        let disk = fs::read_to_string(fixture.root.join(path)).unwrap();

        let (status, body) = put(
            &fixture,
            path,
            DocWriteRequest {
                session_id: None,
                base_version: Some("sha256:not-a-real-version".to_owned()),
                content: None,
                changes: Some(vec![delta(0, 0, "X")]),
            },
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["content"], disk);
        assert_eq!(body["version"], version_hash(&disk));
        let _ = fs::remove_dir_all(&fixture.root);
    }

    #[tokio::test]
    async fn put_publishes_one_file_changed_frame() {
        let fixture = fixture();
        let path = "README.md";
        let absolute = fixture.root.join(path);
        let disk = fs::read_to_string(&absolute).unwrap();
        let version = version_hash(&disk);
        fixture.state.docs_edits.put(version.clone(), &disk);
        let mut subscription = fixture.state.event_bus.subscribe();

        let (status, _) = put(
            &fixture,
            path,
            DocWriteRequest {
                session_id: None,
                base_version: Some(version),
                content: None,
                changes: Some(vec![delta(0, 0, "X")]),
            },
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let envelope = tokio::time::timeout(Duration::from_secs(1), subscription.recv())
            .await
            .expect("a frame")
            .expect("a frame");
        assert_eq!(envelope.event_type, crate::domain::events::FILE_CHANGED);
        assert_eq!(envelope.subject, fixture.workspace_id.to_string());
        assert_eq!(envelope.data["path"], path);
        assert_eq!(envelope.data["source"], "user");
        assert_eq!(envelope.data["outcome"], "applied");
        let _ = fs::remove_dir_all(&fixture.root);
    }

    #[tokio::test]
    async fn put_reports_unchanged_and_refuses_bad_paths() {
        let fixture = fixture();
        let path = "README.md";
        let absolute = fixture.root.join(path);
        let disk = fs::read_to_string(&absolute).unwrap();
        let version = version_hash(&disk);

        let (status, body) = put(
            &fixture,
            path,
            DocWriteRequest {
                session_id: None,
                base_version: Some(version),
                content: Some(disk.clone()),
                changes: None,
            },
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], "unchanged");
        assert_eq!(fs::read_to_string(&absolute).unwrap(), disk);

        // A non-markdown name is refused.
        let (status, _) = put(
            &fixture,
            "src/lib.rs",
            DocWriteRequest {
                session_id: None,
                base_version: None,
                content: Some("x".to_owned()),
                changes: None,
            },
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // A path that escapes the workspace is not found.
        let (status, _) = put(
            &fixture,
            "../escape.md",
            DocWriteRequest {
                session_id: None,
                base_version: None,
                content: Some("x".to_owned()),
                changes: None,
            },
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Neither content nor changes is a bad request.
        let (status, _) = put(
            &fixture,
            path,
            DocWriteRequest {
                session_id: None,
                base_version: Some(version_hash("")),
                content: None,
                changes: None,
            },
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let _ = fs::remove_dir_all(&fixture.root);
    }
}
