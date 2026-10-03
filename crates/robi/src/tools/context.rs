//! Per-session inputs the read tools share.

use std::path::PathBuf;
use std::sync::Arc;

use robi_core::error::ToolError;
use robi_core::ids::SessionId;

use crate::domain::chat_session::service::ChatSessionService;
use crate::domain::file_change::repo::FileChangeRepository;
use crate::index::IndexHub;
use crate::lsp::LspHub;
use crate::workspace::{resolve_path, user_home, PathFilter, ResolvedPath};

/// The workspace and the session whose path rules a tool call reloads.
pub struct ToolContext {
    pub session_id: SessionId,
    pub root: PathBuf,
    pub sessions: Arc<ChatSessionService>,
    pub file_changes: Arc<dyn FileChangeRepository>,
    pub index: Option<Arc<IndexHub>>,
    pub lsp: Arc<LspHub>,
    /// When false, language-server tools are not registered and writes do not
    /// notify a server. Read once, when the actor builds its registry.
    pub lsp_enabled: bool,
    /// Capped shell streams. Absent in tests that do not compress.
    pub originals: Option<Arc<dyn crate::compress::OriginalStore>>,
}

impl ToolContext {
    pub async fn filter(&self) -> Result<PathFilter, ToolError> {
        let session = self
            .sessions
            .get_chat_session(self.session_id)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let mut rules = session.path_rules;
        for skill in self.skills() {
            if skill.directory.as_os_str().is_empty() {
                continue;
            }
            let relative = crate::workspace::workspace_relative(&self.root, &skill.directory);
            rules.allow_read.push(crate::skills::read_allow(&relative));
        }
        PathFilter::for_session(&rules, self.session_id).map_err(ToolError::Failed)
    }

    /// Skills for this workspace, including home and the bundled creator.
    pub fn skills(&self) -> Vec<crate::skills::Skill> {
        crate::skills::scan(super::skill::skill_home().as_deref(), Some(&self.root))
    }

    pub fn resolve(&self, argument: &str) -> Result<ResolvedPath, ToolError> {
        let home = user_home().map_err(|err| ToolError::Failed(err.to_string()))?;
        resolve_path(&self.root, argument, &home)
            .map_err(|err| ToolError::Failed(format!("{err}: {argument}")))
    }

    /// Remember the plan the next agent prompt should re-read.
    pub async fn remember_plan(&self, path: &str) -> Result<(), ToolError> {
        self.sessions
            .set_plan_path(self.session_id, path.to_owned())
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))
    }

    /// Tell a running language server that a write just changed this file.
    ///
    /// A server that is not running is left stopped. The next tool call reads
    /// the disk.
    pub async fn note_lsp(&self, absolute: &std::path::Path, deleted: bool) {
        if !self.lsp_enabled {
            return;
        }
        self.lsp.note_disk(&self.root, absolute, deleted).await;
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
