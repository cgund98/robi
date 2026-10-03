use std::sync::Arc;

use robi_core::ids::SessionId;

use crate::agent::providers::catalog::ModelCatalog;
use crate::agent::providers::config::{ModelId, ReasoningEffort};
use crate::domain::{
    chat_session::{
        model::{
            ChatSession, CreateChatSessionCommand, ModeOverride, ModeOverrideUpdate, ModelConfig,
            UpdateChatSessionCommand,
        },
        repo::ChatSessionRepository,
    },
    error::ServiceError,
    events::{EventBus, EventEnvelope},
    workspace::repo::WorkspaceRepository,
};

/// A chat session title longer than this is rejected before it is written.
pub const CHAT_SESSION_TITLE_MAX_CHARS: usize = 200;

pub struct ChatSessionService {
    pub repository: Arc<dyn ChatSessionRepository>,
    pub workspaces: Arc<dyn WorkspaceRepository>,
    /// Publishes session create, update, and delete. Absent in tests.
    pub events: Option<Arc<EventBus>>,
}

impl ChatSessionService {
    pub async fn create_chat_session(
        &self,
        command: CreateChatSessionCommand,
    ) -> Result<ChatSession, ServiceError> {
        if self
            .workspaces
            .get_workspace(command.workspace_id)
            .await?
            .is_none()
        {
            return Err(ServiceError::NotFound(command.workspace_id.to_string()));
        }
        let title = normalize_new_title(command.title)?;
        let model_config = normalize_model_config(command.model_config)?;
        let session = self
            .repository
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: command.workspace_id,
                title,
                mode: command.mode,
                model_config,
            })
            .await?;
        self.publish(EventEnvelope::session_created(session.id));
        Ok(session)
    }

    pub async fn get_chat_session(&self, id: SessionId) -> Result<ChatSession, ServiceError> {
        self.repository
            .get_chat_session(id)
            .await?
            .ok_or_else(|| ServiceError::NotFound(id.to_string()))
    }

    pub async fn list_chat_sessions(
        &self,
        workspace_id: Option<robi_core::ids::WorkspaceId>,
    ) -> Result<Vec<ChatSession>, ServiceError> {
        self.repository.list_chat_sessions(workspace_id).await
    }

    pub async fn update_chat_session(
        &self,
        mut command: UpdateChatSessionCommand,
    ) -> Result<ChatSession, ServiceError> {
        if let Some(title) = &command.title {
            validate_title(title)?;
        }
        command
            .validate_patterns()
            .map_err(ServiceError::BadRequest)?;
        if let Some(hosts) = &mut command.allow_hosts {
            *hosts = normalize_hosts(hosts)?;
        }
        if let Some(update) = &mut command.model_config {
            normalize_override_update(update.agent.as_mut())?;
            normalize_override_update(update.ask.as_mut())?;
            normalize_override_update(update.plan.as_mut())?;
        }
        if command.is_empty() {
            return self.get_chat_session(command.id).await;
        }
        let id = command.id;
        let session = self.repository.update_chat_session(command).await?;
        self.publish(EventEnvelope::session_updated(id));
        Ok(session)
    }

    /// Name a chat session that does not have a title yet.
    ///
    /// `Ok(None)` means a title was already stored. An empty title is refused
    /// and is not written.
    pub async fn set_title_if_unset(
        &self,
        id: SessionId,
        title: String,
    ) -> Result<Option<ChatSession>, ServiceError> {
        let title = title.trim().to_owned();
        if title.is_empty() {
            return Err(ServiceError::BadRequest(
                "title must not be empty".to_owned(),
            ));
        }
        validate_title(&title)?;
        self.repository.set_title_if_unset(id, title).await
    }

    /// Remember the plan the next agent prompt should re-read.
    pub async fn set_plan_path(&self, id: SessionId, path: String) -> Result<(), ServiceError> {
        let path = path.trim();
        if path.is_empty() {
            return Err(ServiceError::BadRequest(
                "plan_path must not be empty".into(),
            ));
        }
        self.repository.set_plan_path(id, path.to_owned()).await?;
        self.publish(EventEnvelope::session_updated(id));
        Ok(())
    }

    pub async fn delete_chat_session(&self, id: SessionId) -> Result<(), ServiceError> {
        self.repository.delete_chat_session(id).await?;
        self.publish(EventEnvelope::session_deleted(id));
        Ok(())
    }

    fn publish(&self, envelope: EventEnvelope) {
        if let Some(bus) = &self.events {
            bus.publish(envelope);
        }
    }
}

/// `None` and `""` both mean "not named yet", so the model can title the chat session
/// after the first turn.
fn normalize_hosts(hosts: &[String]) -> Result<Vec<String>, ServiceError> {
    hosts
        .iter()
        .map(|host| {
            crate::domain::chat_session::model::normalize_host(host)
                .map_err(ServiceError::BadRequest)
        })
        .collect()
}

fn normalize_new_title(title: Option<String>) -> Result<Option<String>, ServiceError> {
    match title {
        Some(title) if !title.is_empty() => {
            validate_title(&title)?;
            Ok(Some(title))
        }
        _ => Ok(None),
    }
}

fn normalize_model_config(config: ModelConfig) -> Result<ModelConfig, ServiceError> {
    Ok(ModelConfig {
        agent: normalize_override(config.agent)?,
        ask: normalize_override(config.ask)?,
        plan: normalize_override(config.plan)?,
    })
}

fn normalize_override(override_for_mode: ModeOverride) -> Result<ModeOverride, ServiceError> {
    Ok(ModeOverride {
        model: normalize_model(override_for_mode.model)?,
        reasoning_effort: normalize_effort(override_for_mode.reasoning_effort)?,
    })
}

fn normalize_override_update(update: Option<&mut ModeOverrideUpdate>) -> Result<(), ServiceError> {
    let Some(update) = update else {
        return Ok(());
    };
    if let Some(model) = &update.model {
        update.model = Some(normalize_model(model.clone())?);
    }
    if let Some(effort) = &update.reasoning_effort {
        update.reasoning_effort = Some(normalize_effort(effort.clone())?);
    }
    Ok(())
}

fn normalize_model(model: Option<String>) -> Result<Option<String>, ServiceError> {
    let Some(model) = model else {
        return Ok(None);
    };
    if model.is_empty() {
        return Err(ServiceError::BadRequest("model must not be empty".into()));
    }
    let catalog = ModelCatalog::opencode_go();
    if catalog.get(&ModelId::new(model.as_str())).is_none() {
        return Err(ServiceError::BadRequest(format!("unknown model: {model}")));
    }
    Ok(Some(model))
}

fn normalize_effort(effort: Option<String>) -> Result<Option<String>, ServiceError> {
    let Some(effort) = effort else {
        return Ok(None);
    };
    match effort.to_ascii_lowercase().as_str() {
        "low" => Ok(Some(ReasoningEffort::Low.as_str().to_owned())),
        "medium" => Ok(Some(ReasoningEffort::Medium.as_str().to_owned())),
        "high" => Ok(Some(ReasoningEffort::High.as_str().to_owned())),
        _ => Err(ServiceError::BadRequest(format!(
            "reasoning_effort must be low, medium, or high: {effort}"
        ))),
    }
}

fn validate_title(title: &str) -> Result<(), ServiceError> {
    let chars = title.chars().count();
    if chars > CHAT_SESSION_TITLE_MAX_CHARS {
        return Err(ServiceError::BadRequest(format!(
            "title must be at most {CHAT_SESSION_TITLE_MAX_CHARS} characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::Utc;
    use robi_core::ids::{SessionId, WorkspaceId};

    use super::*;
    use crate::domain::{
        chat_session::model::{
            apply_session_update, AgentMode, ModeOverride, ModeOverrideUpdate, ModelConfigUpdate,
            PathRules, UpdateChatSessionCommand,
        },
        workspace::repo::AnyWorkspace,
    };

    struct FakeRepo {
        sessions: Mutex<HashMap<SessionId, ChatSession>>,
    }

    impl FakeRepo {
        fn new() -> Self {
            Self {
                sessions: Mutex::new(HashMap::new()),
            }
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, ChatSession>> {
            self.sessions.lock().expect("fake chat session repo lock")
        }
    }

    #[async_trait]
    impl ChatSessionRepository for FakeRepo {
        async fn create_chat_session(
            &self,
            command: CreateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            let now = Utc::now();
            let session = ChatSession {
                id: SessionId::new(),
                workspace_id: command.workspace_id,
                title: command.title,
                path_rules: PathRules::default(),
                allow_hosts: Vec::new(),
                mcp_allows: Vec::new(),
                mode: command.mode,
                model_config: command.model_config,
                plan_path: None,
                created_at: now,
                updated_at: now,
                last_used_at: now,
            };
            self.lock().insert(session.id, session.clone());
            Ok(session)
        }

        async fn get_chat_session(
            &self,
            id: SessionId,
        ) -> Result<Option<ChatSession>, ServiceError> {
            Ok(self.lock().get(&id).cloned())
        }

        async fn list_chat_sessions(
            &self,
            workspace_id: Option<WorkspaceId>,
        ) -> Result<Vec<ChatSession>, ServiceError> {
            let mut sessions: Vec<_> = self
                .lock()
                .values()
                .filter(|session| match workspace_id {
                    Some(id) => session.workspace_id == id,
                    None => true,
                })
                .cloned()
                .collect();
            sessions.sort_by(|left, right| {
                right
                    .last_used_at
                    .cmp(&left.last_used_at)
                    .then(right.id.cmp(&left.id))
            });
            Ok(sessions)
        }

        async fn update_chat_session(
            &self,
            command: UpdateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            let mut sessions = self.lock();
            let session = sessions
                .get_mut(&command.id)
                .ok_or_else(|| ServiceError::NotFound(command.id.to_string()))?;
            if apply_session_update(session, &command) {
                session.updated_at = Utc::now();
            }
            Ok(session.clone())
        }

        async fn set_title_if_unset(
            &self,
            id: SessionId,
            title: String,
        ) -> Result<Option<ChatSession>, ServiceError> {
            let mut sessions = self.lock();
            let session = sessions
                .get_mut(&id)
                .ok_or_else(|| ServiceError::NotFound(id.to_string()))?;
            if session.title.is_some() {
                return Ok(None);
            }
            session.title = Some(title);
            session.updated_at = Utc::now();
            Ok(Some(session.clone()))
        }

        async fn set_plan_path(&self, id: SessionId, path: String) -> Result<(), ServiceError> {
            let mut sessions = self.lock();
            let session = sessions
                .get_mut(&id)
                .ok_or_else(|| ServiceError::NotFound(id.to_string()))?;
            session.plan_path = Some(path);
            Ok(())
        }

        async fn delete_chat_session(&self, id: SessionId) -> Result<(), ServiceError> {
            if self.lock().remove(&id).is_none() {
                return Err(ServiceError::NotFound(id.to_string()));
            }
            Ok(())
        }
    }

    fn service() -> ChatSessionService {
        ChatSessionService {
            repository: Arc::new(FakeRepo::new()),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
        }
    }

    struct MissingWorkspace;

    #[async_trait]
    impl WorkspaceRepository for MissingWorkspace {
        async fn canonicalize_root(&self, root: &str) -> Result<String, ServiceError> {
            Ok(root.to_string())
        }

        async fn get_workspace(
            &self,
            _id: WorkspaceId,
        ) -> Result<Option<crate::domain::workspace::model::Workspace>, ServiceError> {
            Ok(None)
        }

        async fn get_workspace_by_root(
            &self,
            _root: &str,
        ) -> Result<Option<crate::domain::workspace::model::Workspace>, ServiceError> {
            Ok(None)
        }

        async fn insert_workspace(
            &self,
            _root: &str,
            _name: &str,
        ) -> Result<crate::domain::workspace::model::Workspace, ServiceError> {
            Err(ServiceError::Unknown)
        }

        async fn list_workspaces(
            &self,
        ) -> Result<Vec<crate::domain::workspace::model::Workspace>, ServiceError> {
            Ok(Vec::new())
        }

        async fn delete_workspace(&self, id: WorkspaceId) -> Result<(), ServiceError> {
            Err(ServiceError::NotFound(id.to_string()))
        }

        async fn set_mcp_project_sha256(
            &self,
            _id: WorkspaceId,
            _hash: Option<String>,
        ) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn create_chat_session_is_not_found_for_an_unknown_workspace() {
        let repository = Arc::new(FakeRepo::new());
        let service = ChatSessionService {
            repository: repository.clone(),
            workspaces: Arc::new(MissingWorkspace),
            events: None,
        };
        let workspace_id = WorkspaceId::new();
        let error = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: Some("Notes".into()),
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap_err();

        assert_eq!(error, ServiceError::NotFound(workspace_id.to_string()));
        assert!(repository.lock().is_empty());
    }

    #[tokio::test]
    async fn create_chat_session_stores_the_title() {
        let service = service();
        let workspace_id = WorkspaceId::new();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: Some("Notes".into()),
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();

        assert_eq!(session.workspace_id, workspace_id);
        assert_eq!(session.title.as_deref(), Some("Notes"));
        assert_eq!(
            service
                .get_chat_session(session.id)
                .await
                .unwrap()
                .title
                .as_deref(),
            Some("Notes")
        );
    }

    #[tokio::test]
    async fn create_chat_session_leaves_the_title_unset() {
        let service = service();
        for title in [None, Some(String::new())] {
            let session = service
                .create_chat_session(CreateChatSessionCommand {
                    workspace_id: WorkspaceId::new(),
                    title,
                    mode: AgentMode::Agent,
                    model_config: ModelConfig::default(),
                })
                .await
                .unwrap();
            assert_eq!(session.title, None);
        }
    }

    #[tokio::test]
    async fn create_chat_session_rejects_an_oversized_title() {
        let service = service();
        let title = "a".repeat(CHAT_SESSION_TITLE_MAX_CHARS + 1);
        let error = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: Some(title),
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap_err();

        assert_eq!(
            error,
            ServiceError::BadRequest(format!(
                "title must be at most {CHAT_SESSION_TITLE_MAX_CHARS} characters"
            ))
        );
        assert!(service.list_chat_sessions(None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_chat_session_is_not_found_when_missing() {
        let service = service();
        let id = SessionId::new();
        let error = service.get_chat_session(id).await.unwrap_err();
        assert_eq!(error, ServiceError::NotFound(id.to_string()));
    }

    #[tokio::test]
    async fn list_chat_sessions_filters_by_workspace() {
        let service = service();
        let keep = WorkspaceId::new();
        let drop = WorkspaceId::new();
        service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: keep,
                title: Some("keep".into()),
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();
        service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: drop,
                title: Some("drop".into()),
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();

        let listed = service.list_chat_sessions(Some(keep)).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].workspace_id, keep);
        assert_eq!(listed[0].title.as_deref(), Some("keep"));
    }

    #[tokio::test]
    async fn set_title_if_unset_writes_once() {
        let service = service();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();

        let named = service
            .set_title_if_unset(session.id, "  Parser cleanup  ".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(named.title.as_deref(), Some("Parser cleanup"));
        assert_eq!(named.last_used_at, session.last_used_at);
        assert!(named.updated_at >= session.updated_at);

        assert!(service
            .set_title_if_unset(session.id, "Other".into())
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            service
                .get_chat_session(session.id)
                .await
                .unwrap()
                .title
                .as_deref(),
            Some("Parser cleanup")
        );
    }

    #[tokio::test]
    async fn set_title_if_unset_rejects_an_empty_or_oversized_title() {
        let service = service();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();

        assert_eq!(
            service
                .set_title_if_unset(session.id, "   ".into())
                .await
                .unwrap_err(),
            ServiceError::BadRequest("title must not be empty".into())
        );
        let title = "a".repeat(CHAT_SESSION_TITLE_MAX_CHARS + 1);
        assert!(matches!(
            service.set_title_if_unset(session.id, title).await,
            Err(ServiceError::BadRequest(_))
        ));
        assert_eq!(
            service.get_chat_session(session.id).await.unwrap().title,
            None
        );
    }

    #[tokio::test]
    async fn update_chat_session_is_not_found_when_missing() {
        let service = service();
        let id = SessionId::new();
        let error = service
            .update_chat_session(UpdateChatSessionCommand::rename(id, "Renamed"))
            .await
            .unwrap_err();
        assert_eq!(error, ServiceError::NotFound(id.to_string()));
    }

    #[tokio::test]
    async fn create_stores_the_default_path_rules() {
        let service = service();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();
        assert_eq!(session.path_rules, PathRules::default());
        assert!(session.path_rules.deny_read.is_empty());
        assert!(session.path_rules.deny_write.is_empty());
    }

    #[tokio::test]
    async fn update_rejects_an_invalid_path_pattern() {
        let service = service();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();
        let error = service
            .update_chat_session(UpdateChatSessionCommand {
                id: session.id,
                title: None,
                allow_read: Some(vec!["[".into()]),
                allow_write: None,
                deny_read: None,
                deny_write: None,
                allow_hosts: None,
                mcp_allows: None,
                mode: None,
                model_config: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(error, ServiceError::BadRequest(_)));
        assert_eq!(
            service
                .get_chat_session(session.id)
                .await
                .unwrap()
                .path_rules,
            PathRules::default()
        );
    }

    #[tokio::test]
    async fn update_merges_model_config_and_a_null_key_clears_it() {
        let service = service();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig {
                    agent: ModeOverride {
                        model: Some("glm-5.2".into()),
                        reasoning_effort: Some("low".into()),
                    },
                    ..ModelConfig::default()
                },
            })
            .await
            .unwrap();

        let updated = service
            .update_chat_session(UpdateChatSessionCommand {
                id: session.id,
                title: None,
                allow_read: None,
                allow_write: None,
                deny_read: None,
                deny_write: None,
                allow_hosts: None,
                mcp_allows: None,
                mode: None,
                model_config: Some(ModelConfigUpdate {
                    agent: Some(ModeOverrideUpdate {
                        model: None,
                        reasoning_effort: Some(Some("HIGH".into())),
                    }),
                    ..ModelConfigUpdate::default()
                }),
            })
            .await
            .unwrap();
        assert_eq!(updated.model_config.agent.model.as_deref(), Some("glm-5.2"));
        assert_eq!(
            updated.model_config.agent.reasoning_effort.as_deref(),
            Some("high")
        );

        let cleared = service
            .update_chat_session(UpdateChatSessionCommand {
                id: session.id,
                title: None,
                allow_read: None,
                allow_write: None,
                deny_read: None,
                deny_write: None,
                allow_hosts: None,
                mcp_allows: None,
                mode: None,
                model_config: Some(ModelConfigUpdate {
                    agent: Some(ModeOverrideUpdate {
                        model: Some(None),
                        reasoning_effort: None,
                    }),
                    ..ModelConfigUpdate::default()
                }),
            })
            .await
            .unwrap();
        assert_eq!(
            cleared.model_config,
            ModelConfig {
                agent: ModeOverride {
                    model: None,
                    reasoning_effort: Some("high".into()),
                },
                ..ModelConfig::default()
            }
        );
    }

    #[tokio::test]
    async fn update_rejects_an_unknown_model() {
        let service = service();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();
        let error = service
            .update_chat_session(UpdateChatSessionCommand {
                id: session.id,
                title: None,
                allow_read: None,
                allow_write: None,
                deny_read: None,
                deny_write: None,
                allow_hosts: None,
                mcp_allows: None,
                mode: None,
                model_config: Some(ModelConfigUpdate {
                    ask: Some(ModeOverrideUpdate {
                        model: Some(Some("not-a-model".into())),
                        reasoning_effort: None,
                    }),
                    ..ModelConfigUpdate::default()
                }),
            })
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("unknown model: not-a-model".into())
        );
    }

    #[tokio::test]
    async fn delete_chat_session_removes_it_and_missing_is_not_found() {
        let service = service();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();

        service.delete_chat_session(session.id).await.unwrap();
        assert_eq!(
            service.get_chat_session(session.id).await.unwrap_err(),
            ServiceError::NotFound(session.id.to_string())
        );

        let missing = SessionId::new();
        assert_eq!(
            service.delete_chat_session(missing).await.unwrap_err(),
            ServiceError::NotFound(missing.to_string())
        );
    }
}
