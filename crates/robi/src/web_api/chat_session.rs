use std::collections::HashSet;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use robi_core::ids::{SessionId, WorkspaceId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{
        chat_session::model::{
            AgentMode, ChatSession as DomainChatSession, CreateChatSessionCommand, ModeOverride,
            ModeOverrideUpdate, ModelConfig, ModelConfigUpdate, UpdateChatSessionCommand,
        },
        error::ServiceError,
    },
    web_api::state::AppState,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/chat_sessions",
            get(list_chat_sessions).post(create_chat_session),
        )
        .route(
            "/api/v1/chat_sessions/{id}",
            get(get_chat_session)
                .patch(update_chat_session)
                .delete(delete_chat_session),
        )
        .with_state(state)
}

#[axum::debug_handler]
#[utoipa::path(
    post,
    path = "/api/v1/chat_sessions",
    request_body = CreateChatSession,
    responses(
        (status = 201, description = "Chat session created", body = ChatSession)
    )
)]
pub async fn create_chat_session(
    State(state): State<AppState>,
    Json(payload): Json<CreateChatSession>,
) -> Result<(StatusCode, Json<ChatSession>), ServiceError> {
    let workspace_id = parse_workspace_id(&payload.workspace_id)?;
    let session = state
        .chat_session_service
        .create_chat_session(CreateChatSessionCommand {
            workspace_id,
            title: payload.title,
            mode: parse_mode(payload.mode.as_deref())?,
            model_config: payload
                .model_config
                .map(ModelConfig::from)
                .unwrap_or_default(),
        })
        .await?;
    tracing::info!(
        session = %session.id,
        workspace = %session.workspace_id,
        mode = session.mode.as_str(),
        "chat session created"
    );
    let running = running_sessions(&state).await;

    Ok((StatusCode::CREATED, Json(to_response(session, &running))))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions",
    params(ListChatSessionsQuery),
    responses(
        (status = 200, description = "Chat sessions listed", body = Vec<ChatSession>)
    )
)]
pub async fn list_chat_sessions(
    State(state): State<AppState>,
    Query(query): Query<ListChatSessionsQuery>,
) -> Result<Json<Vec<ChatSession>>, ServiceError> {
    let workspace_id = match query.workspace_id.as_deref() {
        None => None,
        Some(value) => Some(parse_workspace_id(value)?),
    };
    let sessions = state
        .chat_session_service
        .list_chat_sessions(workspace_id)
        .await?;
    let running = running_sessions(&state).await;
    let sessions = sessions
        .into_iter()
        .map(|session| to_response(session, &running))
        .collect();

    Ok(Json(sessions))
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/chat_sessions/{id}",
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 200, description = "Chat session found", body = ChatSession)
    )
)]
pub async fn get_chat_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ChatSession>, ServiceError> {
    let id = parse_chat_session_id(&id)?;
    let session = state.chat_session_service.get_chat_session(id).await?;
    let running = running_sessions(&state).await;
    Ok(Json(to_response(session, &running)))
}

#[axum::debug_handler]
#[utoipa::path(
    patch,
    path = "/api/v1/chat_sessions/{id}",
    request_body = UpdateChatSession,
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 200, description = "Chat session updated", body = ChatSession)
    )
)]
pub async fn update_chat_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateChatSession>,
) -> Result<Json<ChatSession>, ServiceError> {
    let id = parse_chat_session_id(&id)?;
    let session = state
        .chat_session_service
        .update_chat_session(UpdateChatSessionCommand {
            id,
            title: payload.title,
            allow_read: payload.path_allow_read,
            allow_write: payload.path_allow_write,
            deny_read: payload.path_deny_read,
            deny_write: payload.path_deny_write,
            allow_hosts: payload.allow_hosts,
            mcp_allows: None,
            mode: match payload.mode {
                Some(mode) => Some(parse_mode(Some(mode.as_str()))?),
                None => None,
            },
            model_config: payload.model_config.map(ModelConfigUpdate::from),
        })
        .await?;
    let running = running_sessions(&state).await;

    Ok(Json(to_response(session, &running)))
}

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/api/v1/chat_sessions/{id}",
    params(("id" = String, Path, description = "Chat session id")),
    responses(
        (status = 204, description = "Chat session deleted")
    )
)]
pub async fn delete_chat_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ServiceError> {
    let id = parse_chat_session_id(&id)?;
    // Stop a running turn before the row goes. `stop` checks the session exists
    // and returns after that session's actor has exited, so this is 404 for a
    // missing session and a no-op for an idle one.
    state.chat_message_service.stop(id).await?;
    state.chat_session_service.delete_chat_session(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn running_sessions(state: &AppState) -> HashSet<SessionId> {
    state
        .chat_message_service
        .running_session_ids()
        .await
        .into_iter()
        .collect()
}

fn to_response(session: DomainChatSession, running: &HashSet<SessionId>) -> ChatSession {
    ChatSession {
        has_pending_agent: running.contains(&session.id),
        turn_display: session.turn_display.as_str().to_owned(),
        id: session.id.to_string(),
        workspace_id: session.workspace_id.to_string(),
        title: session.title,
        path_allow_read: session.path_rules.allow_read,
        path_allow_write: session.path_rules.allow_write,
        path_deny_read: session.path_rules.deny_read,
        path_deny_write: session.path_rules.deny_write,
        allow_hosts: session.allow_hosts,
        mode: session.mode.as_str().to_owned(),
        model_config: ModelConfigBody::from(session.model_config),
        created_at: rfc3339(session.created_at),
        updated_at: rfc3339(session.updated_at),
        last_used_at: rfc3339(session.last_used_at),
    }
}

pub(crate) fn parse_chat_session_id(value: &str) -> Result<SessionId, ServiceError> {
    parse_uuid(value, "id").map(SessionId::from_uuid)
}

fn parse_workspace_id(value: &str) -> Result<WorkspaceId, ServiceError> {
    parse_uuid(value, "workspace_id").map(WorkspaceId::from_uuid)
}

fn parse_mode(value: Option<&str>) -> Result<AgentMode, ServiceError> {
    AgentMode::parse(value.unwrap_or("agent")).map_err(ServiceError::BadRequest)
}

fn parse_uuid(value: &str, name: &str) -> Result<Uuid, ServiceError> {
    Uuid::parse_str(value).map_err(|_| ServiceError::BadRequest(format!("{name} must be a UUID")))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateChatSession {
    pub workspace_id: String,
    /// Omitted, null, or empty leaves the chat session unnamed. The model writes a
    /// title after the first turn. Longer than
    /// [`CHAT_SESSION_TITLE_MAX_CHARS`](crate::domain::chat_session::service::CHAT_SESSION_TITLE_MAX_CHARS)
    /// is rejected.
    #[serde(default)]
    pub title: Option<String>,
    /// `ask`, `plan`, or `agent`. Omitted starts in agent mode.
    #[serde(default)]
    pub mode: Option<String>,
    /// Per-mode session overrides. Omitted keys inherit that mode's setting.
    #[serde(default)]
    pub model_config: Option<ModelConfigBody>,
}

/// Model and effort for one mode. Absent keys inherit the setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ModeOverrideBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

impl ModeOverrideBody {
    fn is_empty(&self) -> bool {
        self.model.is_none() && self.reasoning_effort.is_none()
    }
}

impl From<ModeOverride> for ModeOverrideBody {
    fn from(override_for_mode: ModeOverride) -> Self {
        Self {
            model: override_for_mode.model,
            reasoning_effort: override_for_mode.reasoning_effort,
        }
    }
}

impl From<ModeOverrideBody> for ModeOverride {
    fn from(body: ModeOverrideBody) -> Self {
        Self {
            model: body.model,
            reasoning_effort: body.reasoning_effort,
        }
    }
}

/// Stored per-mode overrides. An absent mode inherits its setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ModelConfigBody {
    #[serde(default, skip_serializing_if = "ModeOverrideBody::is_empty")]
    pub agent: ModeOverrideBody,
    #[serde(default, skip_serializing_if = "ModeOverrideBody::is_empty")]
    pub ask: ModeOverrideBody,
    #[serde(default, skip_serializing_if = "ModeOverrideBody::is_empty")]
    pub plan: ModeOverrideBody,
}

impl From<ModelConfig> for ModelConfigBody {
    fn from(config: ModelConfig) -> Self {
        Self {
            agent: config.agent.into(),
            ask: config.ask.into(),
            plan: config.plan.into(),
        }
    }
}

impl From<ModelConfigBody> for ModelConfig {
    fn from(body: ModelConfigBody) -> Self {
        Self {
            agent: body.agent.into(),
            ask: body.ask.into(),
            plan: body.plan.into(),
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateChatSession {
    /// When set, replaces the title. Omitted leaves the stored title alone.
    #[serde(default)]
    pub title: Option<String>,
    /// Extra read-allow regexes, appended after the built-in list. The furthest match wins; at the same end byte, more literals win.
    #[serde(default)]
    pub path_allow_read: Option<Vec<String>>,
    /// Regexes. Ranked the same way as read allows. A parent match does not outrank a more specific write deny.
    #[serde(default)]
    pub path_allow_write: Option<Vec<String>>,
    /// Regexes matched against the workspace-relative path. A match denies a read.
    #[serde(default)]
    pub path_deny_read: Option<Vec<String>>,
    /// Regexes matched against the workspace-relative path. A match denies a write.
    #[serde(default)]
    pub path_deny_write: Option<Vec<String>>,
    /// Hostnames `web_fetch` may call without another approval. A present list replaces the stored list.
    #[serde(default)]
    pub allow_hosts: Option<Vec<String>>,
    /// `ask`, `plan`, or `agent`. Omitted leaves the stored mode.
    #[serde(default)]
    pub mode: Option<String>,
    /// Merge into the stored per-mode overrides. A null key clears that override. An omitted key stays.
    #[serde(default)]
    pub model_config: Option<ModelConfigPatch>,
}

/// One mode's model and effort patch.
///
/// `None` means the key was omitted. `Some(None)` clears it. `Some(Some)` sets it.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct ModeOverridePatch {
    /// Catalog model id. Null clears the session override.
    #[serde(default, deserialize_with = "some_or_absent")]
    pub model: Option<Option<String>>,
    /// `low`, `medium`, or `high`. Null clears the session override.
    #[serde(default, deserialize_with = "some_or_absent")]
    pub reasoning_effort: Option<Option<String>>,
}

impl From<ModeOverridePatch> for ModeOverrideUpdate {
    fn from(patch: ModeOverridePatch) -> Self {
        Self {
            model: patch.model,
            reasoning_effort: patch.reasoning_effort,
        }
    }
}

/// Per-mode session overrides. An omitted mode stays as stored.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct ModelConfigPatch {
    #[serde(default)]
    pub agent: Option<ModeOverridePatch>,
    #[serde(default)]
    pub ask: Option<ModeOverridePatch>,
    #[serde(default)]
    pub plan: Option<ModeOverridePatch>,
}

/// `None` when the key is absent. `Some(None)` when the key is null.
fn some_or_absent<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl From<ModelConfigPatch> for ModelConfigUpdate {
    fn from(patch: ModelConfigPatch) -> Self {
        Self {
            agent: patch.agent.map(ModeOverrideUpdate::from),
            ask: patch.ask.map(ModeOverrideUpdate::from),
            plan: patch.plan.map(ModeOverrideUpdate::from),
        }
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListChatSessionsQuery {
    pub workspace_id: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ChatSession {
    pub id: String,
    pub workspace_id: String,
    /// Absent until the model names the chat session after the first turn, or a
    /// create/rename supplies one.
    pub title: Option<String>,
    pub path_allow_read: Vec<String>,
    pub path_allow_write: Vec<String>,
    pub path_deny_read: Vec<String>,
    pub path_deny_write: Vec<String>,
    /// Hosts this session may fetch without another approval card.
    pub allow_hosts: Vec<String>,
    /// `ask`, `plan`, or `agent`.
    pub mode: String,
    pub model_config: ModelConfigBody,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: String,
    /// True while this session's actor is running. Read from memory, not the database.
    pub has_pending_agent: bool,
    /// Display summary of the latest turn: `idle`, `pending`, `awaiting_approval`, or `failed`.
    pub turn_display: String,
}

fn rfc3339(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use axum::extract::{Path, State};
    use robi_core::ids::{SessionId, ToolCallId};
    use robi_core::store::MessageStore;
    use uuid::Uuid;

    use crate::{
        adapters::{
            chat_message::SqliteMessageStore, chat_session::repo::SqliteChatSessionRepository,
            sqlite, workspace::repo::SqliteWorkspaceRepository,
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
            settings::{memory::MemorySettingsStore, store::SettingsStore, SettingsService},
            workspace::service::WorkspaceService,
        },
        web_api::state::AppState,
    };

    use super::{delete_chat_session, ModelConfigPatch};

    #[test]
    fn a_null_key_clears_and_an_omitted_key_stays() {
        let cleared: ModelConfigPatch =
            serde_json::from_str(r#"{"ask":{"reasoning_effort":null}}"#).unwrap();
        assert!(cleared.agent.is_none());
        let ask = cleared.ask.unwrap();
        assert_eq!(ask.model, None);
        assert_eq!(ask.reasoning_effort, Some(None));

        let set: ModelConfigPatch =
            serde_json::from_str(r#"{"plan":{"model":"glm-5.2"}}"#).unwrap();
        let plan = set.plan.unwrap();
        assert_eq!(plan.model, Some(Some("glm-5.2".into())));
        assert_eq!(plan.reasoning_effort, None);
    }

    /// Records every `stop` it was asked to perform.
    struct RecordingRuntime {
        stopped: Mutex<Vec<SessionId>>,
    }

    impl RecordingRuntime {
        fn new() -> Self {
            Self {
                stopped: Mutex::new(Vec::new()),
            }
        }

        fn stopped(&self) -> Vec<SessionId> {
            self.stopped.lock().expect("runtime lock").clone()
        }
    }

    #[async_trait]
    impl ChatRuntime for RecordingRuntime {
        async fn submit(
            &self,
            _session: SessionId,
            _instruction: String,
            _images: Vec<robi_core::message::ImageAttachment>,
            _files: Vec<robi_core::message::FileAttachment>,
        ) -> Result<SubmitOutcome, ServiceError> {
            Ok(SubmitOutcome::Accepted)
        }

        async fn running_session_ids(&self) -> Vec<SessionId> {
            Vec::new()
        }

        async fn decide(
            &self,
            _session: SessionId,
            _call: ToolCallId,
            _reject: Option<String>,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn stop(&self, session: SessionId) -> Result<(), ServiceError> {
            self.stopped.lock().expect("runtime lock").push(session);
            Ok(())
        }

        async fn compact(&self, _session: SessionId) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    async fn state_with(runtime: Arc<dyn ChatRuntime>) -> AppState {
        let url = format!(
            "sqlite://file:robi-delete-{}?mode=memory&cache=shared",
            Uuid::now_v7().simple()
        );
        let pool = Arc::new(sqlite::init_pool(&url).await.expect("pool"));
        let workspaces = Arc::new(SqliteWorkspaceRepository::new(Arc::clone(&pool)));
        let workspace_service = Arc::new(WorkspaceService {
            repository: workspaces.clone(),
            asset_cleaner: None,
        });
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(SqliteChatSessionRepository::new(Arc::clone(&pool))),
            workspaces,
            events: None,
            plan_cleaner: None,
        });
        let store: Arc<dyn MessageStore> = Arc::new(SqliteMessageStore::new(
            Arc::clone(&pool),
            crate::adapters::session_blobs::SessionBlobs::new(
                std::env::temp_dir().join(format!("robi-delete-blobs-{}", Uuid::now_v7().simple())),
            ),
        ));
        AppState {
            workspace_service,
            chat_session_service: Arc::clone(&sessions),
            chat_message_service: Arc::new(ChatMessageService {
                sessions,
                runtime,
                store,
            }),
            settings_service: Arc::new(SettingsService {
                store: Arc::new(MemorySettingsStore::new()) as Arc<dyn SettingsStore>,
            }),
            event_bus: Arc::new(EventBus::new()),
            file_changes: Arc::new(
                crate::adapters::file_change::repo::SqliteFileChangeRepository::new(Arc::clone(
                    &pool,
                )),
            ),
            index: Arc::new(crate::agent::index::IndexHub::new(
                std::env::temp_dir(),
                Arc::new(EventBus::new()),
                Arc::new(robi_index::FakeEmbedder::new(4)),
            )),
            mcp: None,
            originals: Arc::new(crate::agent::compress::MemoryOriginals::default()),
            image_source: Arc::new(crate::adapters::chat_image_store::MemoryImageStore::new()),
            docs_edits: Arc::new(crate::agent::docs::DocsEditCache::default()),
        }
    }

    #[tokio::test]
    async fn delete_stops_the_running_actor_before_removing_the_row() {
        let runtime = Arc::new(RecordingRuntime::new());
        let state = state_with(runtime.clone()).await;
        let root = std::env::temp_dir().join(format!("robi-delete-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let workspace = state
            .workspace_service
            .open_workspace(root.to_str().unwrap())
            .await
            .unwrap();
        let session = state
            .chat_session_service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: workspace.workspace.id,
                title: None,
                mode: AgentMode::Agent,
                model_config: Default::default(),
            })
            .await
            .unwrap();

        let status = delete_chat_session(State(state.clone()), Path(session.id.to_string()))
            .await
            .unwrap();
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
        assert_eq!(runtime.stopped(), vec![session.id]);
        assert!(state
            .chat_session_service
            .get_chat_session(session.id)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn delete_of_a_missing_session_is_not_found_and_does_not_stop() {
        let runtime = Arc::new(RecordingRuntime::new());
        let state = state_with(runtime.clone()).await;
        let missing = SessionId::new();

        let error = delete_chat_session(State(state), Path(missing.to_string()))
            .await
            .unwrap_err();
        assert_eq!(error, ServiceError::NotFound(missing.to_string()));
        assert!(runtime.stopped().is_empty());
    }
}
