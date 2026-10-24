//! The shared fixture for the tool tests.
//!
//! The edit, read, shell, and subagent tests all need a real session with real
//! path rules, so the builder lives here and is compiled in every test build.

use std::sync::Arc;

use robi_core::ids::SessionId;
use robi_core::tool::ToolRun;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::context::ToolContext;
use crate::adapters::{
    chat_session::repo::SqliteChatSessionRepository, file_change::repo::SqliteFileChangeRepository,
    sqlite, workspace::repo::SqliteWorkspaceRepository,
};
use crate::domain::{
    chat_session::{
        model::{AgentMode, CreateChatSessionCommand},
        service::ChatSessionService,
    },
    workspace::service::WorkspaceService,
};

pub(crate) struct Harness {
    pub(crate) ctx: Arc<ToolContext>,
    pub(crate) root: std::path::PathBuf,
    /// Read by the `edit-tools` tests only.
    #[cfg_attr(not(feature = "edit-tools"), allow(dead_code))]
    pub(crate) session_id: SessionId,
}

pub(crate) async fn harness() -> Harness {
    let url = format!(
        "sqlite://file:robi-edit-{}?mode=memory&cache=shared",
        Uuid::now_v7().simple()
    );
    let pool = Arc::new(sqlite::init_pool(&url).await.expect("pool"));
    let root = std::env::temp_dir().join(format!("robi-edit-{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&root).unwrap();
    let workspaces = Arc::new(SqliteWorkspaceRepository::new(Arc::clone(&pool)));
    let opened = WorkspaceService {
        repository: workspaces.clone(),
        asset_cleaner: None,
    }
    .open_workspace(root.to_str().unwrap())
    .await
    .unwrap();
    let sessions = Arc::new(ChatSessionService {
        repository: Arc::new(SqliteChatSessionRepository::new(Arc::clone(&pool))),
        workspaces,
        events: None,
        session_cleaner: None,
    });
    let chat = sessions
        .create_chat_session(CreateChatSessionCommand {
            workspace_id: opened.workspace.id,
            title: None,
            mode: AgentMode::Agent,
            model_config: crate::domain::chat_session::model::ModelConfig::default(),
        })
        .await
        .unwrap();
    let canonical = std::fs::canonicalize(&root).unwrap();
    let ctx = Arc::new(ToolContext {
        session_id: chat.id,
        workspace_id: chat.workspace_id,
        root: canonical,
        sessions,
        file_changes: Arc::new(SqliteFileChangeRepository::new(pool)),
        originals: None,
        settings: None,
        events: None,
    });
    Harness {
        ctx,
        root,
        session_id: chat.id,
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg_attr(
    not(any(feature = "edit-tools", feature = "read-tools")),
    allow(dead_code)
)]
pub(crate) fn run() -> ToolRun {
    ToolRun::new(CancellationToken::new())
}
