//! Per-session inputs the read tools share.

use std::path::PathBuf;
use std::sync::Arc;

use robi_core::error::ToolError;
use robi_core::ids::{SessionId, WorkspaceId};

use crate::agent::index::IndexHub;
use crate::agent::lsp::LspHub;
use crate::agent::workspace::{resolve_path, user_home, PathFilter, ResolvedPath};
use crate::domain::chat_session::service::ChatSessionService;
use crate::domain::events::EventBus;
use crate::domain::file_change::repo::FileChangeRepository;
use crate::domain::settings::store::SettingsStore;

/// The workspace and the session whose path rules a tool call reloads.
pub struct ToolContext {
    pub session_id: SessionId,
    /// The workspace this session belongs to, the `file_changed` subject.
    pub workspace_id: WorkspaceId,
    pub root: PathBuf,
    pub sessions: Arc<ChatSessionService>,
    pub file_changes: Arc<dyn FileChangeRepository>,
    pub index: Option<Arc<IndexHub>>,
    pub lsp: Arc<LspHub>,
    /// When false, language-server tools are not registered and writes do not
    /// notify a server. Read once, when the actor builds its registry.
    pub lsp_enabled: bool,
    /// Capped shell streams. Absent in tests that do not compress.
    pub originals: Option<Arc<dyn crate::agent::compress::OriginalStore>>,
    /// Live settings. Each path-filter and sandbox build reads
    /// `path_allow_read` and `path_allow_write` from here. Absent in tests.
    pub settings: Option<Arc<dyn SettingsStore>>,
    /// The event bus. An edit tool announces the path it wrote here. Absent in
    /// tests that do not publish.
    pub events: Option<Arc<EventBus>>,
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
            let relative =
                crate::agent::workspace::workspace_relative(&self.root, &skill.directory);
            rules
                .allow_read
                .push(crate::agent::skills::read_allow(&relative));
        }
        self.append_configured_allows(&mut rules).await;
        PathFilter::for_session(&rules, self.session_id).map_err(ToolError::Failed)
    }

    /// Settings paths, with `~/` expanded, appended to the session allow lists.
    ///
    /// Reads the store on this call. A write that landed after this agent was
    /// built is included.
    pub async fn append_configured_allows(
        &self,
        rules: &mut crate::domain::chat_session::model::PathRules,
    ) {
        let Some(settings) = &self.settings else {
            return;
        };
        let Ok(home) = user_home() else {
            return;
        };
        let read = setting_text(settings, crate::domain::settings::keys::PATH_ALLOW_READ).await;
        let write = setting_text(settings, crate::domain::settings::keys::PATH_ALLOW_WRITE).await;
        let entries = setting_text(settings, crate::domain::settings::keys::PATH_ENTRIES).await;
        rules
            .allow_read
            .extend(super::grant::allows_from_setting(&self.root, &home, &read));
        rules.allow_read.extend(super::grant::allows_from_setting(
            &self.root, &home, &entries,
        ));
        rules
            .allow_write
            .extend(super::grant::allows_from_setting(&self.root, &home, &write));
    }

    /// Skills for this workspace, including home and the bundled creator.
    pub fn skills(&self) -> Vec<crate::agent::skills::Skill> {
        crate::agent::skills::scan(super::skill::skill_home().as_deref(), Some(&self.root))
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

    /// The stored string for `key`, or empty when it is absent or the read fails.
    /// A stored whole number from 1 through `max`. Absent or unusable is `default`.
    pub async fn bounded_u32(&self, key: &str, default: u32, max: u32) -> u32 {
        let raw = self.setting_value(key).await;
        let value = if raw.is_empty() {
            None
        } else {
            Some(raw.as_str())
        };
        crate::domain::settings::keys::parse_bounded(value, default, max)
    }

    pub async fn setting_value(&self, key: &str) -> String {
        let Some(settings) = &self.settings else {
            return String::new();
        };
        setting_text(settings, key).await
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

async fn setting_text(settings: &Arc<dyn SettingsStore>, key: &str) -> String {
    match settings.get(key).await {
        Ok(Some(setting)) => setting.value,
        Ok(None) => String::new(),
        Err(err) => {
            tracing::error!(%key, %err, "failed to read a path allow setting");
            String::new()
        }
    }
}
