//! `GET /api/v1/chat_sessions/{id}/review`.
//!
//! The session's baselines compared with the files on disk.

use std::path::Path;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    agent::review::{
        decide_review, review_for_session, FileStatus, Hunk, ReviewDecision, ReviewFile,
        ReviewLine, ReviewLineKind,
    },
    domain::error::ServiceError,
    web_api::{chat_session::parse_chat_session_id, state::AppState},
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/chat_sessions/{id}/review",
            get(get_session_review).post(decide_session_review),
        )
        .with_state(state)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions/{id}/review",
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 200, description = "Files this session has changed", body = SessionReview)
    )
)]
pub async fn get_session_review(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<SessionReview>, ServiceError> {
    let id = parse_chat_session_id(&id)?;
    let session = state.chat_session_service.get_chat_session(id).await?;
    let workspace = state
        .workspace_service
        .get_workspace(session.workspace_id)
        .await?;
    let files =
        review_for_session(state.file_changes.as_ref(), Path::new(&workspace.root), id).await?;
    Ok(Json(SessionReview {
        files: files.into_iter().map(ReviewFileBody::from).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/chat_sessions/{id}/review",
    params(("id" = String, Path, description = "Chat session id")),
    request_body = DecideReview,
    responses((status = 204, description = "The file or hunk was approved or rejected"))
)]
pub async fn decide_session_review(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<DecideReview>,
) -> Result<StatusCode, ServiceError> {
    let id = parse_chat_session_id(&id)?;
    let session = state.chat_session_service.get_chat_session(id).await?;
    let workspace = state
        .workspace_service
        .get_workspace(session.workspace_id)
        .await?;
    let decision = match body.decision.as_str() {
        "approve" => ReviewDecision::Approve,
        "reject" => ReviewDecision::Reject,
        _ => {
            return Err(ServiceError::BadRequest(
                "decision must be approve or reject".to_owned(),
            ));
        }
    };
    decide_review(
        state.file_changes.as_ref(),
        Path::new(&workspace.root),
        id,
        &body.path,
        body.hunk_id.as_deref(),
        decision,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DecideReview {
    pub path: String,
    /// `approve` keeps the change. `reject` puts the baseline lines back.
    pub decision: String,
    /// One hunk. Omitted applies `decision` to the whole file.
    #[serde(default)]
    pub hunk_id: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SessionReview {
    pub files: Vec<ReviewFileBody>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ReviewFileBody {
    pub path: String,
    pub status: ReviewStatus,
    pub additions: u32,
    pub deletions: u32,
    /// Body before this session's first change of the path.
    pub baseline: String,
    /// Body on disk now. Empty when the file is gone.
    pub current: String,
    pub lines: Vec<ReviewLineBody>,
    pub hunks: Vec<ReviewHunkBody>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ReviewHunkBody {
    pub id: String,
    /// 0-based index of the first baseline line.
    pub old_start: u32,
    pub old_count: u32,
    /// 0-based index of the first current line.
    pub new_start: u32,
    pub new_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Added,
    Deleted,
    Modified,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ReviewLineBody {
    pub kind: ReviewLineStatus,
    pub text: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLineStatus {
    Context,
    Delete,
    Insert,
    Gap,
}

impl From<ReviewFile> for ReviewFileBody {
    fn from(file: ReviewFile) -> Self {
        Self {
            path: file.path,
            status: ReviewStatus::from(file.status),
            additions: file.additions,
            deletions: file.deletions,
            baseline: file.baseline,
            current: file.current,
            lines: file.lines.into_iter().map(ReviewLineBody::from).collect(),
            hunks: file.hunks.iter().map(ReviewHunkBody::from).collect(),
        }
    }
}

impl From<&Hunk> for ReviewHunkBody {
    fn from(hunk: &Hunk) -> Self {
        Self {
            id: hunk.id.clone(),
            old_start: hunk.old_start as u32,
            old_count: hunk.old.len() as u32,
            new_start: hunk.new_start as u32,
            new_count: hunk.new.len() as u32,
        }
    }
}

impl From<FileStatus> for ReviewStatus {
    fn from(status: FileStatus) -> Self {
        match status {
            FileStatus::Added => Self::Added,
            FileStatus::Deleted => Self::Deleted,
            FileStatus::Modified => Self::Modified,
        }
    }
}

impl From<ReviewLine> for ReviewLineBody {
    fn from(line: ReviewLine) -> Self {
        Self {
            kind: ReviewLineStatus::from(line.kind),
            text: line.text,
            old_line: line.old_line,
            new_line: line.new_line,
        }
    }
}

impl From<ReviewLineKind> for ReviewLineStatus {
    fn from(kind: ReviewLineKind) -> Self {
        match kind {
            ReviewLineKind::Context => Self::Context,
            ReviewLineKind::Delete => Self::Delete,
            ReviewLineKind::Insert => Self::Insert,
            ReviewLineKind::Gap => Self::Gap,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::extract::{Path, State};
    use robi_core::error::StoreError;
    use robi_core::ids::{MessageId, SessionId, WorkspaceId};
    use robi_core::message::Message;
    use robi_core::store::MessageStore;
    use uuid::Uuid;

    use crate::{
        adapters::{
            chat_session::repo::SqliteChatSessionRepository,
            file_change::repo::SqliteFileChangeRepository, sqlite,
            workspace::repo::SqliteWorkspaceRepository,
        },
        domain::{
            chat_message::{
                runtime::{ChatRuntime, SubmitOutcome},
                service::ChatMessageService,
            },
            chat_session::{
                model::{AgentMode, CreateChatSessionCommand},
                service::ChatSessionService,
            },
            error::ServiceError,
            events::EventBus,
            file_change::repo::FileChangeRepository,
            settings::{memory::MemorySettingsStore, store::SettingsStore, SettingsService},
            workspace::service::WorkspaceService,
        },
        web_api::state::AppState,
    };

    use super::{get_session_review, ReviewLineStatus, ReviewStatus};

    struct Idle;

    #[async_trait]
    impl ChatRuntime for Idle {
        async fn submit(
            &self,
            _session: SessionId,
            _instruction: String,
            _images: Vec<robi_core::message::ImageAttachment>,
        ) -> Result<SubmitOutcome, ServiceError> {
            Ok(SubmitOutcome::Accepted)
        }

        async fn running_session_ids(&self) -> Vec<SessionId> {
            Vec::new()
        }

        async fn decide(
            &self,
            _session: SessionId,
            _call: robi_core::ids::ToolCallId,
            _reject: Option<String>,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn stop(&self, _session: SessionId) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn compact(&self, _session: SessionId) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    #[async_trait]
    impl MessageStore for Idle {
        fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
            SessionId::new()
        }

        fn has_session(&self, _session: SessionId) -> bool {
            false
        }

        async fn messages(&self, _session: SessionId) -> Result<Vec<Message>, StoreError> {
            Ok(Vec::new())
        }

        async fn message(
            &self,
            _session: SessionId,
            _id: MessageId,
        ) -> Result<Option<Message>, StoreError> {
            Ok(None)
        }

        async fn append(&self, _session: SessionId, _message: Message) -> Result<(), StoreError> {
            Ok(())
        }

        async fn update(&self, _session: SessionId, _message: Message) -> Result<(), StoreError> {
            Ok(())
        }

        async fn replace_prefix(
            &self,
            _session: SessionId,
            _delete: &[MessageId],
            _summary: Message,
        ) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn the_handler_returns_changed_files_and_skips_an_unchanged_baseline() {
        let url = format!(
            "sqlite://file:robi-review-{}?mode=memory&cache=shared",
            Uuid::now_v7().simple()
        );
        let pool = Arc::new(sqlite::init_pool(&url).await.expect("pool"));
        let root = std::env::temp_dir().join(format!("robi-review-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let workspaces = Arc::new(SqliteWorkspaceRepository::new(Arc::clone(&pool)));
        let workspace_service = Arc::new(WorkspaceService {
            repository: workspaces.clone(),
            asset_cleaner: None,
        });
        let opened = workspace_service
            .open_workspace(root.to_str().unwrap())
            .await
            .unwrap();
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(SqliteChatSessionRepository::new(Arc::clone(&pool))),
            workspaces,
            events: None,
            plan_cleaner: None,
        });
        let session = sessions
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: opened.workspace.id,
                title: None,
                mode: AgentMode::Agent,
                model_config: Default::default(),
            })
            .await
            .unwrap();
        let canonical = std::fs::canonicalize(&root).unwrap();
        std::fs::write(canonical.join("a.txt"), "one\nthree\n").unwrap();
        std::fs::write(canonical.join("same.txt"), "keep\n").unwrap();
        let file_changes: Arc<dyn FileChangeRepository> =
            Arc::new(SqliteFileChangeRepository::new(pool));
        file_changes
            .record_baseline(session.id, "a.txt", "one\ntwo\n", false)
            .await
            .unwrap();
        file_changes
            .record_baseline(session.id, "same.txt", "keep\n", false)
            .await
            .unwrap();
        let state = AppState {
            workspace_service,
            chat_session_service: Arc::clone(&sessions),
            chat_message_service: Arc::new(ChatMessageService {
                sessions,
                runtime: Arc::new(Idle),
                store: Arc::new(Idle),
            }),
            settings_service: Arc::new(SettingsService {
                store: Arc::new(MemorySettingsStore::new()) as Arc<dyn SettingsStore>,
            }),
            event_bus: Arc::new(EventBus::new()),
            file_changes,
            index: Arc::new(crate::agent::index::IndexHub::new(
                std::env::temp_dir(),
                Arc::new(EventBus::new()),
                Arc::new(robi_index::FakeEmbedder::new(4)),
            )),
            mcp: None,
            originals: Arc::new(crate::agent::compress::MemoryOriginals::default()),
            image_source: Arc::new(crate::adapters::chat_image_store::MemoryImageStore::new()),
        };

        let body = get_session_review(State(state), Path(session.id.to_string()))
            .await
            .unwrap()
            .0;
        assert_eq!(body.files.len(), 1);
        let file = &body.files[0];
        assert_eq!(file.path, "a.txt");
        assert!(matches!(file.status, ReviewStatus::Modified));
        assert_eq!(file.additions, 1);
        assert_eq!(file.deletions, 1);
        assert_eq!(file.baseline, "one\ntwo\n");
        assert_eq!(file.current, "one\nthree\n");
        assert!(file
            .lines
            .iter()
            .any(|line| line.kind == ReviewLineStatus::Delete && line.text == "two"));
        assert!(file
            .lines
            .iter()
            .any(|line| line.kind == ReviewLineStatus::Insert && line.text == "three"));
        assert_eq!(file.hunks.len(), 1);
        assert!(!file.hunks[0].id.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }
}
