use std::collections::HashMap;
use std::sync::Arc;

use robi_core::ids::{SessionId, WorkspaceId};

use crate::agent::providers::catalog::ModelCatalog;
use crate::agent::providers::config::{ModelId, ReasoningEffort, OPENCODE_GO_PREFIX};
use crate::domain::{
    chat_session::{
        data::SessionDataCleaner,
        lease::{Locks, Resource, Scope},
        model::{
            ChatSession, CreateChatSessionCommand, ModeOverride, ModeOverrideUpdate, ModelConfig,
            TurnDisplay, UpdateChatSessionCommand,
        },
        repo::ChatSessionRepository,
    },
    error::ServiceError,
    events::{EventBus, EventEnvelope},
    workspace::repo::WorkspaceRepository,
};

/// The display value a restart should store.
///
/// A waiting call wins. Otherwise a stored failure stays, and a `pending`
/// left by a crash becomes `idle`.
pub fn reconciled_turn_display(
    stored: TurnDisplay,
    messages: &[robi_core::message::Message],
) -> TurnDisplay {
    let waiting = messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .any(|call| call.is_pending_approval() && call.needs_execution());
    if waiting {
        TurnDisplay::AwaitingApproval
    } else if stored == TurnDisplay::Failed {
        TurnDisplay::Failed
    } else {
        TurnDisplay::Idle
    }
}

/// A chat session title longer than this is rejected before it is written.
pub const CHAT_SESSION_TITLE_MAX_CHARS: usize = 200;

/// Sessions kept per workspace when the process starts.
pub const MAX_SESSIONS_PER_WORKSPACE: usize = 50;

pub struct ChatSessionService {
    pub repository: Arc<dyn ChatSessionRepository>,
    pub workspaces: Arc<dyn WorkspaceRepository>,
    /// Publishes session create, update, and delete. Absent in tests.
    pub events: Option<Arc<EventBus>>,
    /// Removes the data a session keeps outside its row, such as blobs. Plan
    /// directories live in the workspace and are left alone.
    /// Absent in tests. Best effort: a failure is logged and the delete still
    /// succeeds, because the row is already gone.
    pub session_cleaner: Option<Arc<dyn SessionDataCleaner>>,
    /// Cross-process ownership of a session's blob file.
    ///
    /// Required rather than optional, unlike the two above: an absent lock would
    /// mean unenforced ownership on every path that matters, and an `Option`
    /// reads as "tests do not need it" instead of "tests must say so".
    /// `MemoryLocks` is what a test passes.
    pub locks: Arc<dyn Locks>,
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
        workspace_id: Option<WorkspaceId>,
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
        if let Some(session) = self
            .repository
            .set_title_if_unset(id, title.clone())
            .await?
        {
            self.publish(EventEnvelope::session_title_changed(id, title));
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    /// Store the display summary and publish it on `robi.session.v1.updated`.
    pub async fn set_turn_display(
        &self,
        id: SessionId,
        display: TurnDisplay,
    ) -> Result<(), ServiceError> {
        self.repository.set_turn_display(id, display).await?;
        self.publish(EventEnvelope::session_turn_display(id, display));
        Ok(())
    }

    /// Repair `turn_display` from the transcript after a process restart.
    ///
    /// A waiting tool call forces `awaiting_approval`. A stored `failed` stays
    /// when nothing is waiting. Anything else, including a leftover `pending`,
    /// becomes `idle`. No event is published.
    ///
    /// Only unsettled sessions are read, which is the sessions a process was
    /// mid-turn in when it died: the write that stores a display also settles the
    /// row, so a start no longer reads every transcript in the database.
    ///
    /// Skips any session another process holds, and returns how many. The owner is
    /// the authority on its own turn, and repairing underneath it would write
    /// `idle` over a running turn; the next start repairs the row if the owner exits
    /// first.
    ///
    /// The display is written even when it already matches, because that write is
    /// what settles the row. [`reconciled_turn_display`] never returns `pending`, so
    /// a settled write always follows.
    pub async fn reconcile_turn_displays(
        &self,
        store: &dyn robi_core::store::MessageStore,
    ) -> Result<usize, ServiceError> {
        let sessions = self.repository.list_unsettled_chat_sessions().await?;
        let mut skipped = 0;
        for session in sessions {
            // Before the read, because the read is the work being skipped: a
            // session another process is driving has a display that process will
            // write itself.
            let Ok(_guard) = self
                .locks
                .try_acquire(&Resource::Session(session.id), Scope::Turn)
            else {
                skipped += 1;
                continue;
            };
            // Without results: every field the repair reads is in the message body,
            // so the session's `blobs.redb` is never opened and no handle is left
            // behind.
            let messages = store
                .messages_without_results(session.id)
                .await
                .map_err(|error| {
                    tracing::error!(%error, "failed to read messages for turn display");
                    ServiceError::Unknown
                })?;
            let next = reconciled_turn_display(session.turn_display, &messages);
            self.repository.set_turn_display(session.id, next).await?;
        }
        Ok(skipped)
    }

    /// Delete the row and the data outside it.
    ///
    /// Refused while another process holds the session: the delete unlinks a
    /// directory that process may have open, which is a data-loss race rather
    /// than a stale render.
    pub async fn delete_chat_session(&self, id: SessionId) -> Result<(), ServiceError> {
        let guard = self
            .locks
            .try_acquire(&Resource::Session(id), Scope::Turn)
            .map_err(|error| error.conflict())?;
        self.repository.delete_chat_session(id).await?;
        if let Some(cleaner) = &self.session_cleaner {
            if let Err(err) = cleaner.remove_session_data(id).await {
                tracing::warn!(session = %id, %err, "failed to remove session data");
            }
        }
        self.publish(EventEnvelope::session_deleted(id));
        // Held until the row and the blobs are gone, so a second process cannot
        // start driving a session that is half deleted.
        drop(guard);
        Ok(())
    }

    /// Drop sessions past [`MAX_SESSIONS_PER_WORKSPACE`], least recently used first.
    ///
    /// The list is newest-first, so the tail of each workspace is the oldest.
    /// Each removal goes through [`Self::delete_chat_session`].
    pub async fn prune_excess_sessions(&self) -> Result<usize, ServiceError> {
        let sessions = self.repository.list_chat_sessions(None).await?;
        let mut by_workspace: HashMap<WorkspaceId, Vec<SessionId>> = HashMap::new();
        for session in sessions {
            by_workspace
                .entry(session.workspace_id)
                .or_default()
                .push(session.id);
        }
        let mut removed = 0;
        for ids in by_workspace.values() {
            if ids.len() <= MAX_SESSIONS_PER_WORKSPACE {
                continue;
            }
            for id in ids[MAX_SESSIONS_PER_WORKSPACE..].iter().rev() {
                match self.delete_chat_session(*id).await {
                    Ok(()) => removed += 1,
                    // In use by another process. Keep it and let a later start
                    // prune: aborting startup over it would be worse, and the
                    // workspace is only one over its limit.
                    Err(ServiceError::Conflict(_)) => {
                        tracing::info!(%id, "kept a session another process is using");
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(removed)
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
    // A11: the union of both catalogs is the namespace. A bare legacy id is read
    // as OpenCode Go and rewritten to its prefixed form, so a new write is always
    // prefixed while an old session keeps working.
    let catalog = ModelCatalog::all();
    if catalog.get(&ModelId::new(model.as_str())).is_some() {
        return Ok(Some(model));
    }
    let prefixed = format!("{OPENCODE_GO_PREFIX}{model}");
    if catalog.get(&ModelId::new(prefixed.as_str())).is_some() {
        return Ok(Some(prefixed));
    }
    Err(ServiceError::BadRequest(format!("unknown model: {model}")))
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
    use crate::domain::chat_session::lease::MemoryLocks;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::Utc;
    use robi_core::ids::{SessionId, WorkspaceId};

    use super::*;
    use crate::domain::chat_session::lease::{LeaseError, LeaseGuard, LeaseHolder, Scope};
    use crate::domain::{
        chat_session::model::{
            apply_session_update, AgentMode, ModeOverride, ModeOverrideUpdate, ModelConfigUpdate,
            PathRules, TurnDisplay, UpdateChatSessionCommand,
        },
        workspace::repo::AnyWorkspace,
    };

    struct FakeRepo {
        sessions: Mutex<HashMap<SessionId, ChatSession>>,
        /// The `turn_settled` column. `insert` leaves a row unsettled, like a
        /// session a turn was starting in.
        settled: Mutex<HashSet<SessionId>>,
    }

    impl FakeRepo {
        fn new() -> Self {
            Self {
                sessions: Mutex::new(HashMap::new()),
                settled: Mutex::new(HashSet::new()),
            }
        }

        fn is_settled(&self, id: SessionId) -> bool {
            self.settled
                .lock()
                .expect("fake settled lock")
                .contains(&id)
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, ChatSession>> {
            self.sessions.lock().expect("fake chat session repo lock")
        }

        fn insert(&self, session: ChatSession) {
            self.lock().insert(session.id, session);
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
                turn_display: TurnDisplay::Idle,
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

        async fn list_unsettled_chat_sessions(&self) -> Result<Vec<ChatSession>, ServiceError> {
            let mut sessions: Vec<_> = self
                .lock()
                .values()
                .filter(|session| !self.is_settled(session.id))
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

        async fn set_turn_display(
            &self,
            id: SessionId,
            display: TurnDisplay,
        ) -> Result<(), ServiceError> {
            let mut sessions = self.lock();
            let session = sessions
                .get_mut(&id)
                .ok_or_else(|| ServiceError::NotFound(id.to_string()))?;
            session.turn_display = display;
            let mut settled = self.settled.lock().expect("fake settled lock");
            if display.is_settled() {
                settled.insert(id);
            } else {
                settled.remove(&id);
            }
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
            session_cleaner: None,
            locks: Arc::new(MemoryLocks::new()),
        }
    }

    /// Records the sessions it was asked to clean. `fail` flips the result.
    struct FakeCleaner {
        removed: Mutex<Vec<SessionId>>,
        fail: bool,
    }

    impl FakeCleaner {
        fn new() -> Self {
            Self {
                removed: Mutex::new(Vec::new()),
                fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                removed: Mutex::new(Vec::new()),
                fail: true,
            }
        }

        fn removed(&self) -> Vec<SessionId> {
            self.removed.lock().expect("fake cleaner lock").clone()
        }
    }

    #[async_trait]
    impl SessionDataCleaner for FakeCleaner {
        async fn remove_session_data(&self, session: SessionId) -> Result<(), String> {
            self.removed
                .lock()
                .expect("fake cleaner lock")
                .push(session);
            if self.fail {
                return Err("cleaner failed".into());
            }
            Ok(())
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
            locks: Arc::new(MemoryLocks::new()),
            repository: repository.clone(),
            workspaces: Arc::new(MissingWorkspace),
            events: None,
            session_cleaner: None,
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
                        model: Some("ocg_glm-5.2".into()),
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
        assert_eq!(
            updated.model_config.agent.model.as_deref(),
            Some("ocg_glm-5.2")
        );
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

    fn session_at(workspace_id: WorkspaceId, last_used_at: chrono::DateTime<Utc>) -> ChatSession {
        ChatSession {
            id: SessionId::new(),
            workspace_id,
            title: None,
            path_rules: PathRules::default(),
            allow_hosts: Vec::new(),
            mcp_allows: Vec::new(),
            mode: AgentMode::Agent,
            model_config: ModelConfig::default(),
            turn_display: TurnDisplay::Idle,
            created_at: last_used_at,
            updated_at: last_used_at,
            last_used_at,
        }
    }

    #[test]
    fn reconcile_clears_a_leftover_pending_and_keeps_a_failure() {
        assert_eq!(
            reconciled_turn_display(TurnDisplay::Pending, &[]),
            TurnDisplay::Idle
        );
        assert_eq!(
            reconciled_turn_display(TurnDisplay::Failed, &[]),
            TurnDisplay::Failed
        );
        let waiting = robi_core::message::Message::assistant_with_tool_calls(
            "",
            vec![robi_core::message::ToolCall::new(
                "shell",
                serde_json::json!({}),
            )],
        );
        assert_eq!(
            reconciled_turn_display(TurnDisplay::Idle, &[waiting]),
            TurnDisplay::AwaitingApproval
        );
    }

    #[tokio::test]
    async fn prune_excess_sessions_drops_the_least_recently_used() {
        let repo = Arc::new(FakeRepo::new());
        let full = WorkspaceId::new();
        let small = WorkspaceId::new();
        let start = Utc::now();
        let mut oldest = None;
        let mut newest = None;
        for index in 0..=MAX_SESSIONS_PER_WORKSPACE {
            let session = session_at(full, start + chrono::Duration::seconds(index as i64));
            if index == 0 {
                oldest = Some(session.id);
            }
            if index == MAX_SESSIONS_PER_WORKSPACE {
                newest = Some(session.id);
            }
            repo.insert(session);
        }
        let kept_small = session_at(small, start);
        let small_id = kept_small.id;
        repo.insert(kept_small);

        let service = ChatSessionService {
            repository: repo,
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            session_cleaner: None,
            locks: Arc::new(MemoryLocks::new()),
        };
        assert_eq!(service.prune_excess_sessions().await.unwrap(), 1);

        let remaining = service.list_chat_sessions(None).await.unwrap();
        let full_ids: Vec<_> = remaining
            .iter()
            .filter(|session| session.workspace_id == full)
            .map(|session| session.id)
            .collect();
        assert_eq!(full_ids.len(), MAX_SESSIONS_PER_WORKSPACE);
        assert!(!full_ids.contains(&oldest.unwrap()));
        assert!(full_ids.contains(&newest.unwrap()));
        assert!(remaining.iter().any(|session| session.id == small_id));
    }

    /// Refuses the sessions it is told about, and grants the rest.
    struct HeldByAnother {
        held: Vec<SessionId>,
    }

    impl Locks for HeldByAnother {
        fn try_acquire(&self, resource: &Resource, scope: Scope) -> Result<LeaseGuard, LeaseError> {
            if let Resource::Session(id) = resource {
                if self.held.contains(id) {
                    return Err(LeaseError::held(resource, LeaseHolder::new(4812, scope)));
                }
            }
            Ok(LeaseGuard::noop())
        }

        fn holder(&self, resource: &Resource) -> Option<LeaseHolder> {
            match resource {
                Resource::Session(id) if self.held.contains(id) => {
                    Some(LeaseHolder::new(4812, Scope::Turn))
                }
                _ => None,
            }
        }
    }

    #[tokio::test]
    async fn reconcile_skips_a_session_another_process_holds() {
        let repo = Arc::new(FakeRepo::new());
        let workspace = WorkspaceId::new();
        let mut held = session_at(workspace, Utc::now());
        held.turn_display = TurnDisplay::Pending;
        let free = session_at(workspace, Utc::now());
        repo.insert(held.clone());
        repo.insert(free.clone());

        // The held session would have been repaired from `pending` to `idle`.
        let store = Arc::new(StubStore {
            messages: vec![(held.id, Vec::new()), (free.id, Vec::new())],
            reads: Mutex::new(Vec::new()),
        });
        let service = ChatSessionService {
            repository: repo.clone(),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            session_cleaner: None,
            locks: Arc::new(HeldByAnother {
                held: vec![held.id],
            }),
        };

        let skipped = service
            .reconcile_turn_displays(store.as_ref())
            .await
            .unwrap();
        assert_eq!(skipped, 1, "one session was left alone");

        let after = service.list_chat_sessions(None).await.unwrap();
        let held_after = after.iter().find(|s| s.id == held.id).unwrap();
        assert_eq!(
            held_after.turn_display,
            TurnDisplay::Pending,
            "the held session keeps whatever the owner wrote"
        );
        let free_after = after.iter().find(|s| s.id == free.id).unwrap();
        assert_eq!(free_after.turn_display, TurnDisplay::Idle);
    }

    #[tokio::test]
    async fn reconcile_reads_a_session_once_and_not_again() {
        let repo = Arc::new(FakeRepo::new());
        let workspace = WorkspaceId::new();
        let session = session_at(workspace, Utc::now());
        repo.insert(session.clone());
        let service = ChatSessionService {
            repository: repo.clone(),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            session_cleaner: None,
            locks: Arc::new(MemoryLocks),
        };
        let store = Arc::new(StubStore {
            messages: vec![(session.id, Vec::new())],
            reads: Mutex::new(Vec::new()),
        });

        service
            .reconcile_turn_displays(store.as_ref())
            .await
            .unwrap();
        assert_eq!(
            store.reads.lock().expect("read log").as_slice(),
            &[session.id],
            "an unsettled session's transcript is read"
        );

        service
            .reconcile_turn_displays(store.as_ref())
            .await
            .unwrap();
        assert_eq!(
            store.reads.lock().expect("read log").len(),
            1,
            "the write that repaired it also took it off the list"
        );
    }

    #[tokio::test]
    async fn a_turn_in_flight_is_unsettled_and_its_end_is_settled() {
        let repo = Arc::new(FakeRepo::new());
        let service = ChatSessionService {
            repository: repo.clone(),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            session_cleaner: None,
            locks: Arc::new(MemoryLocks),
        };
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();

        service
            .set_turn_display(session.id, TurnDisplay::Pending)
            .await
            .unwrap();
        assert_eq!(
            repo.list_unsettled_chat_sessions().await.unwrap().len(),
            1,
            "a starting turn is what puts a session back on the list"
        );

        service
            .set_turn_display(session.id, TurnDisplay::AwaitingApproval)
            .await
            .unwrap();
        assert!(
            repo.list_unsettled_chat_sessions()
                .await
                .unwrap()
                .is_empty(),
            "the actor stopped, so the row already matches its transcript"
        );
    }

    #[tokio::test]
    async fn prune_keeps_a_session_another_process_holds() {
        let repo = Arc::new(FakeRepo::new());
        let workspace = WorkspaceId::new();
        let start = Utc::now() - chrono::Duration::hours(1);
        let mut held = None;
        for index in 0..=MAX_SESSIONS_PER_WORKSPACE {
            let session = session_at(workspace, start + chrono::Duration::seconds(index as i64));
            if index == 0 {
                held = Some(session.id);
            }
            repo.insert(session);
        }
        let held = held.expect("the oldest session");

        let service = ChatSessionService {
            repository: repo,
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            session_cleaner: None,
            locks: Arc::new(HeldByAnother { held: vec![held] }),
        };

        // The oldest is over the limit and would be deleted, but it is in use.
        assert_eq!(
            service.prune_excess_sessions().await.unwrap(),
            0,
            "a held session is retained rather than aborting startup"
        );
        let remaining = service.list_chat_sessions(None).await.unwrap();
        assert!(remaining.iter().any(|session| session.id == held));
        assert!(remaining.len() > MAX_SESSIONS_PER_WORKSPACE);
    }

    /// A store whose transcripts are fixed, so a reconcile has something to read.
    struct StubStore {
        messages: Vec<(SessionId, Vec<robi_core::message::Message>)>,
        /// Every session whose transcript was asked for, in order.
        reads: Mutex<Vec<SessionId>>,
    }

    #[async_trait::async_trait]
    impl robi_core::store::MessageStore for StubStore {
        fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
            SessionId::new()
        }
        fn has_session(&self, session: SessionId) -> bool {
            self.messages.iter().any(|(id, _)| *id == session)
        }
        async fn messages(
            &self,
            session: SessionId,
        ) -> Result<Vec<robi_core::message::Message>, robi_core::error::StoreError> {
            self.reads.lock().expect("read log").push(session);
            Ok(self
                .messages
                .iter()
                .find(|(id, _)| *id == session)
                .map(|(_, messages)| messages.clone())
                .unwrap_or_default())
        }
        async fn messages_without_results(
            &self,
            session: SessionId,
        ) -> Result<Vec<robi_core::message::Message>, robi_core::error::StoreError> {
            self.messages(session).await
        }
        async fn message(
            &self,
            _session: SessionId,
            _id: robi_core::ids::MessageId,
        ) -> Result<Option<robi_core::message::Message>, robi_core::error::StoreError> {
            Ok(None)
        }
        async fn append(
            &self,
            _session: SessionId,
            _message: robi_core::message::Message,
        ) -> Result<(), robi_core::error::StoreError> {
            Ok(())
        }
        async fn update(
            &self,
            _session: SessionId,
            _message: robi_core::message::Message,
        ) -> Result<(), robi_core::error::StoreError> {
            Ok(())
        }
        async fn replace_prefix(
            &self,
            _session: SessionId,
            _delete: &[robi_core::ids::MessageId],
            _summary: robi_core::message::Message,
        ) -> Result<(), robi_core::error::StoreError> {
            Ok(())
        }
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
    async fn update_accepts_a_legacy_bare_id_and_stores_the_prefixed_form() {
        // A11: an old session wrote a bare id; it reads as OpenCode Go and is
        // rewritten to the prefixed form on the way in.
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
                        model: Some(Some("glm-5.3".into())),
                        reasoning_effort: None,
                    }),
                    ..ModelConfigUpdate::default()
                }),
            })
            .await
            .unwrap();
        assert_eq!(
            updated.model_config.agent.model.as_deref(),
            Some("ocg_glm-5.3")
        );
    }

    #[tokio::test]
    async fn update_accepts_an_anthropic_model() {
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
                        model: Some(Some("ant_claude-sonnet-5-5".into())),
                        reasoning_effort: None,
                    }),
                    ..ModelConfigUpdate::default()
                }),
            })
            .await
            .unwrap();
        assert_eq!(
            updated.model_config.agent.model.as_deref(),
            Some("ant_claude-sonnet-5-5")
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

    #[tokio::test]
    async fn delete_removes_plan_files_for_the_deleted_session() {
        let cleaner = Arc::new(FakeCleaner::new());
        let service = ChatSessionService {
            repository: Arc::new(FakeRepo::new()),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            session_cleaner: Some(cleaner.clone()),
            locks: Arc::new(MemoryLocks::new()),
        };
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
        assert_eq!(cleaner.removed(), vec![session.id]);

        // A miss does not reach the cleaner.
        let missing = SessionId::new();
        assert!(service.delete_chat_session(missing).await.is_err());
        assert_eq!(cleaner.removed(), vec![session.id]);
    }

    #[tokio::test]
    async fn a_cleaner_failure_does_not_fail_the_delete() {
        let service = ChatSessionService {
            repository: Arc::new(FakeRepo::new()),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            session_cleaner: Some(Arc::new(FakeCleaner::failing())),
            locks: Arc::new(MemoryLocks::new()),
        };
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
        assert!(service.get_chat_session(session.id).await.is_err());
    }
}
