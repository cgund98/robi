//! `GET` the markdown files of a workspace, and `GET` one file's content.
//!
//! The docs viewer is a workspace-scoped tree of markdown pages. The listing is
//! paths only; the content is fetched per selection. The walk respects
//! `.gitignore` and skips hidden entries, so `node_modules/` and `target/` stay
//! out without a special case.

use std::path::{Path, PathBuf};

use axum::{
    extract::{Path as AxumPath, State},
    routing::get,
    Json, Router,
};
use ignore::WalkBuilder;
use serde::Serialize;

use crate::{
    agent::workspace::workspace_relative,
    domain::error::ServiceError,
    web_api::{state::AppState, workspace::parse_workspace_id},
};

/// The listing stops after this many files. A larger docs set still opens: the
/// tree is navigational, and this cap only bounds one response.
const MAX_FILES: usize = 500;

/// One page is rendered whole. A bigger file is cut and marked, so a generated
/// page cannot stall the renderer.
const MAX_DOC_BYTES: usize = 512 * 1024;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/workspaces/{id}/docs", get(list_docs))
        .route("/api/v1/workspaces/{id}/docs/{*path}", get(get_doc))
        .with_state(state)
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocsListing {
    pub files: Vec<DocEntry>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocEntry {
    /// Workspace-relative, `/` separated.
    pub path: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocContent {
    pub path: String,
    pub content: String,
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{id}/docs",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "Markdown files in this workspace, gitignore respected", body = DocsListing)
    )
)]
pub async fn list_docs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DocsListing>, ServiceError> {
    let id = parse_workspace_id(&id)?;
    let workspace = state.workspace_service.get_workspace(id).await?;
    let root = PathBuf::from(&workspace.root);
    let files = tokio::task::spawn_blocking(move || list_markdown_files(&root))
        .await
        .map_err(|_| ServiceError::Unknown)??;
    Ok(Json(DocsListing {
        files: files.into_iter().map(|path| DocEntry { path }).collect(),
    }))
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
    Ok(Json(DocContent { path, content }))
}

/// Markdown files under `root`, workspace-relative and `/` separated, sorted.
///
/// Hidden entries and gitignored paths are skipped. Symlinks are not followed.
/// The result stops at [`MAX_FILES`].
pub fn list_markdown_files(root: &Path) -> Result<Vec<String>, ServiceError> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .follow_links(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true);
    let mut files = Vec::new();
    for entry in builder.build() {
        if files.len() >= MAX_FILES {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        if !is_markdown(path) {
            continue;
        }
        files.push(workspace_relative(root, path));
    }
    files.sort();
    Ok(files)
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

    use super::{list_markdown_files, read_markdown};
    use crate::domain::error::ServiceError;

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
        fs::write(root.join(".gitignore"), "secret.md\ntarget/\n").unwrap();
        fs::write(root.join("README.md"), "# Readme\n").unwrap();
        fs::write(root.join("docs/a.md"), "# A\n\nbody\n").unwrap();
        fs::write(root.join("docs/nested/b.markdown"), "b\n").unwrap();
        fs::write(root.join("docs/a.txt"), "not markdown\n").unwrap();
        fs::write(root.join("secret.md"), "ignored\n").unwrap();
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
}
