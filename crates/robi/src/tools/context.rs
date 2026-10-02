//! Per-session inputs the read tools share.

use std::path::PathBuf;
use std::sync::Arc;

use robi_core::error::ToolError;
use robi_core::ids::SessionId;

use crate::domain::chat_session::service::ChatSessionService;
use crate::domain::file_change::repo::FileChangeRepository;
use crate::workspace::{resolve_path, user_home, PathFilter, ResolvedPath};

/// The workspace and the session whose path rules a tool call reloads.
pub struct ToolContext {
    pub session_id: SessionId,
    pub root: PathBuf,
    pub sessions: Arc<ChatSessionService>,
    pub file_changes: Arc<dyn FileChangeRepository>,
}

impl ToolContext {
    pub async fn filter(&self) -> Result<PathFilter, ToolError> {
        let session = self
            .sessions
            .get_chat_session(self.session_id)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        PathFilter::compile(&session.path_rules).map_err(ToolError::Failed)
    }

    pub fn resolve(&self, argument: &str) -> Result<ResolvedPath, ToolError> {
        let home = user_home().map_err(|err| ToolError::Failed(err.to_string()))?;
        resolve_path(&self.root, argument, &home)
            .map_err(|err| ToolError::Failed(format!("{err}: {argument}")))
    }
}

pub fn display_path(path: &ResolvedPath) -> String {
    if path.relative.is_empty() {
        ".".to_owned()
    } else {
        path.relative.clone()
    }
}

pub fn denied(path: &ResolvedPath) -> ToolError {
    ToolError::Failed(format!("path is not allowed: {}", display_path(path)))
}
