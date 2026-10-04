//! One actor per chat session.
//!
//! The runtime stores the pieces an [`Agent`] is built from. Each actor
//! constructs its own agent when it starts and drops it when the session goes
//! idle. A newer instruction cancels the in-flight turn and replaces any
//! instruction that has not started.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use robi_core::agent::Agent;
use robi_core::config::LoopConfig;
use robi_core::error::TurnOutcome;
use robi_core::event::EventSink;
use robi_core::ids::{SessionId, ToolCallId};
use robi_core::model::Model;
use robi_core::store::MessageStore;
use robi_core::tool::ToolRegistry;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use crate::{
    adapters::{model_source::ModelSource, session_title::title_completed_turn},
    domain::{
        chat_message::runtime::{ChatRuntime, SubmitOutcome},
        chat_session::{
            model::{AgentMode, ModeOverride},
            service::ChatSessionService,
        },
        error::ServiceError,
        events::EventBus,
        file_change::repo::FileChangeRepository,
        settings::store::SettingsStore,
    },
};

/// Process-wide pieces a session actor needs. The agent is not one of them.
///
/// `models` is read when an actor starts. A turn that is already running keeps
/// the model that actor was built with.
pub struct AgentFactory {
    pub store: Arc<dyn MessageStore>,
    pub events: Arc<dyn EventSink>,
    pub models: Arc<dyn ModelSource>,
    pub tools: Arc<ToolRegistry>,
    pub config: LoopConfig,
    /// Chat session rows. Absent in tests that do not name sessions.
    pub sessions: Option<Arc<ChatSessionService>>,
    /// Baselines for files this session changes. Required when `sessions` is set.
    pub file_changes: Option<Arc<dyn FileChangeRepository>>,
    /// Brave, or a fake in tests. `web_search` calls this.
    pub search: Arc<dyn crate::agent::web::SearchEngine>,
    /// Publishes `robi.session.v1.updated` after a title is stored. Absent when
    /// there is no bus.
    pub bus: Option<Arc<EventBus>>,
    /// Workspace semantic index. Absent in tests that do not search.
    pub index: Option<Arc<crate::agent::index::IndexHub>>,
    /// Language servers shared by sessions on one workspace. Absent in tests
    /// that do not call them; those sessions get a hub of their own.
    pub lsp: Option<Arc<crate::agent::lsp::LspHub>>,
    /// Global settings. Absent in tests, which leave language-server tools on.
    pub settings: Option<Arc<dyn SettingsStore>>,
    /// MCP host. Absent in tests. Agent mode attaches its tools before the model runs.
    pub mcp: Option<Arc<crate::agent::mcp::McpHub>>,
    /// Capped shell streams. Absent in tests that do not compress.
    pub originals: Option<Arc<dyn crate::agent::compress::OriginalStore>>,
}

impl AgentFactory {
    fn build(
        &self,
        session: SessionId,
        model: Arc<dyn Model>,
        tools: Arc<ToolRegistry>,
        max_iterations: u32,
    ) -> Agent {
        let agent = Agent::new(
            Arc::clone(&self.store),
            Arc::clone(&self.events),
            model,
            tools,
            self.config.with_max_iterations(max_iterations),
        );
        match &self.originals {
            Some(store) => agent.with_compressor(Arc::new(
                crate::agent::compress::ShellCompressor::new(Arc::clone(store), session),
            )),
            None => agent,
        }
    }

    /// The tools this session's actor will run.
    ///
    /// A factory without chat session rows keeps the registry it was given.
    /// Otherwise the actor gets the read tools closed over this session.
    async fn session_registry(
        &self,
        session: SessionId,
    ) -> Result<
        (
            Arc<ToolRegistry>,
            Option<std::path::PathBuf>,
            AgentMode,
            ModeOverride,
            Option<String>,
        ),
        ServiceError,
    > {
        let Some(sessions) = &self.sessions else {
            return Ok((
                Arc::clone(&self.tools),
                None,
                AgentMode::Agent,
                ModeOverride::default(),
                None,
            ));
        };
        let chat = sessions.get_chat_session(session).await?;
        let mode = chat.mode;
        let choice = chat.model_config.for_mode(mode).clone();
        let workspace = sessions
            .workspaces
            .get_workspace(chat.workspace_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound(chat.workspace_id.to_string()))?;
        let root = std::path::PathBuf::from(&workspace.root);
        let root = root.canonicalize().unwrap_or(root);
        let file_changes = self.file_changes.clone().ok_or_else(|| {
            tracing::error!("session tools require a file change repository");
            ServiceError::Unknown
        })?;
        let registry = ToolRegistry::new();
        let lsp_enabled = self.lsp_enabled().await?;
        let ctx = Arc::new(crate::agent::tools::ToolContext {
            session_id: session,
            root: root.clone(),
            sessions: Arc::clone(sessions),
            file_changes,
            index: self.index.clone(),
            lsp: self
                .lsp
                .clone()
                .unwrap_or_else(crate::agent::lsp::LspHub::new),
            lsp_enabled,
            originals: self.originals.clone(),
            settings: self.settings.clone(),
        });
        let models = Arc::new(crate::agent::tools::SessionChildModels {
            session_id: session,
            sessions: Arc::clone(sessions),
            models: Arc::clone(&self.models),
        });
        let search_approval = self
            .approval_required(crate::domain::settings::keys::WEB_SEARCH_APPROVAL)
            .await?;
        let fetch_approval = self
            .approval_required(crate::domain::settings::keys::WEB_FETCH_APPROVAL)
            .await?;
        crate::agent::tools::register_tools_for_mode(
            &registry,
            ctx,
            mode,
            models,
            Arc::clone(&self.search),
            search_approval,
            fetch_approval,
        )
        .map_err(|err| {
            tracing::error!(%session, %err, "failed to register tools");
            ServiceError::Unknown
        })?;
        if mode == AgentMode::Agent {
            if let Some(mcp) = &self.mcp {
                let allows = Arc::new(crate::agent::mcp::SessionAllows {
                    sessions: Arc::clone(sessions),
                    session,
                });
                mcp.attach(
                    workspace.id,
                    &root,
                    workspace.mcp_project_sha256.as_deref(),
                    &registry,
                    allows,
                    &crate::agent::mcp::RmcpOpener,
                )
                .await;
            }
        }
        tracing::info!(%session, mode = mode.as_str(), "prepared the session actor");
        Ok((Arc::new(registry), Some(root), mode, choice, chat.plan_path))
    }

    async fn lsp_enabled(&self) -> Result<bool, ServiceError> {
        let Some(settings) = &self.settings else {
            return Ok(true);
        };
        let stored = settings.get(crate::domain::settings::keys::LSP).await?;
        Ok(crate::domain::settings::keys::lsp_enabled(
            stored.as_ref().map(|setting| setting.value.as_str()),
        ))
    }

    async fn approval_required(&self, key: &str) -> Result<bool, ServiceError> {
        let Some(settings) = &self.settings else {
            return Ok(true);
        };
        let stored = settings.get(key).await?;
        Ok(crate::domain::settings::keys::approval_required(
            stored.as_ref().map(|setting| setting.value.as_str()),
        ))
    }

    async fn max_iterations(&self) -> Result<u32, ServiceError> {
        let Some(settings) = &self.settings else {
            return Ok(self.config.max_iterations);
        };
        let stored = settings
            .get(crate::domain::settings::keys::MAX_ITERATIONS)
            .await?;
        Ok(crate::domain::settings::keys::parse_iterations(
            stored.as_ref().map(|setting| setting.value.as_str()),
            self.config.max_iterations,
        ))
    }
}

enum Work {
    Instruction {
        instruction: String,
        images: Vec<robi_core::message::ImageAttachment>,
    },
    Decision {
        call: ToolCallId,
        reject: Option<String>,
    },
}

#[derive(Default)]
struct Slot {
    cancel: Option<CancellationToken>,
    pending: Option<Work>,
    running: bool,
    /// Bumped each time an actor is started, so stop waits for that actor only.
    generation: u64,
}

struct Slots {
    map: Mutex<HashMap<SessionId, Slot>>,
    /// Signaled when an actor clears `running`.
    idle: Notify,
}

/// Serializes `user_input` for each chat session.
pub struct SerializedChatRuntime {
    factory: AgentFactory,
    slots: Arc<Slots>,
}

impl SerializedChatRuntime {
    pub fn new(factory: AgentFactory) -> Self {
        Self {
            factory,
            slots: Arc::new(Slots {
                map: Mutex::new(HashMap::new()),
                idle: Notify::new(),
            }),
        }
    }

    fn spawn_actor(
        &self,
        session: SessionId,
        model: Arc<dyn Model>,
        tools: Arc<ToolRegistry>,
        max_iterations: u32,
    ) {
        let agent = self
            .factory
            .build(session, Arc::clone(&model), tools, max_iterations);
        let slots = Arc::clone(&self.slots);
        let store = Arc::clone(&self.factory.store);
        let sessions = self.factory.sessions.clone();
        let bus = self.factory.bus.clone();
        tracing::info!(%session, "session actor started");
        tokio::spawn(async move {
            run_actor(agent, model, store, sessions, bus, slots, session).await;
        });
    }

    async fn start_actor(
        &self,
        session: SessionId,
        instruction: String,
        images: Vec<robi_core::message::ImageAttachment>,
        model: Arc<dyn Model>,
        tools: Arc<ToolRegistry>,
    ) -> Result<SubmitOutcome, ServiceError> {
        let mut slots = self.slots.map.lock().await;
        let slot = slots.entry(session).or_default();
        if slot.running {
            replace_pending(slot, session, instruction, images);
            return Ok(SubmitOutcome::Accepted);
        }
        slot.running = true;
        slot.generation += 1;
        slot.pending = Some(Work::Instruction {
            instruction,
            images,
        });
        drop(slots);
        let max_iterations = self.factory.max_iterations().await?;
        self.spawn_actor(session, model, tools, max_iterations);
        Ok(SubmitOutcome::Accepted)
    }

    async fn start_decision(
        &self,
        session: SessionId,
        call: ToolCallId,
        reject: Option<String>,
        model: Arc<dyn Model>,
        tools: Arc<ToolRegistry>,
    ) -> Result<(), ServiceError> {
        let mut slots = self.slots.map.lock().await;
        let slot = slots.entry(session).or_default();
        if slot.running {
            tracing::warn!(%session, "decision refused because the session is running");
            return Err(ServiceError::Conflict("chat session is running".into()));
        }
        slot.running = true;
        slot.generation += 1;
        slot.pending = Some(Work::Decision { call, reject });
        drop(slots);
        let max_iterations = self.factory.max_iterations().await?;
        self.spawn_actor(session, model, tools, max_iterations);
        Ok(())
    }

    async fn actor_running(&self, session: SessionId) -> bool {
        self.slots
            .map
            .lock()
            .await
            .get(&session)
            .is_some_and(|slot| slot.running)
    }
}

#[async_trait]
impl ChatRuntime for SerializedChatRuntime {
    async fn submit(
        &self,
        session: SessionId,
        instruction: String,
        images: Vec<robi_core::message::ImageAttachment>,
    ) -> Result<SubmitOutcome, ServiceError> {
        if interrupt_if_running(&self.slots, session, &instruction, images.clone()).await {
            return Ok(SubmitOutcome::Accepted);
        }

        // Resolve before the slot is marked running, so a missing key does not
        // leave an actor that will never start. A paused turn is settled inside
        // `user_input`, which rejects the calls still waiting.
        let (tools, workspace, mode, choice, plan_path) =
            self.factory.session_registry(session).await?;

        if self.actor_running(session).await {
            let mut slots = self.slots.map.lock().await;
            let slot = slots.entry(session).or_default();
            if slot.running {
                replace_pending(slot, session, instruction, images);
                return Ok(SubmitOutcome::Accepted);
            }
        }
        let model = match self
            .factory
            .models
            .model(Arc::clone(&tools), workspace, mode, choice, plan_path)
            .await
        {
            Ok(model) => model,
            Err(error) => {
                tracing::error!(%session, %error, "failed to build the session model");
                return Err(error);
            }
        };
        self.start_actor(session, instruction, images, model, tools)
            .await
    }

    async fn decide(
        &self,
        session: SessionId,
        call: ToolCallId,
        reject: Option<String>,
    ) -> Result<(), ServiceError> {
        if self.actor_running(session).await {
            tracing::warn!(%session, "decision refused because the session is running");
            return Err(ServiceError::Conflict("chat session is running".into()));
        }
        let (tools, workspace, mode, choice, plan_path) =
            self.factory.session_registry(session).await?;
        let model = match self
            .factory
            .models
            .model(Arc::clone(&tools), workspace, mode, choice, plan_path)
            .await
        {
            Ok(model) => model,
            Err(error) => {
                tracing::error!(%session, %error, "failed to build the session model");
                return Err(error);
            }
        };
        let approved = reject.is_none();
        tracing::info!(%session, %call, approved, "tool decision accepted");
        self.start_decision(session, call, reject, model, tools)
            .await
    }

    async fn running_session_ids(&self) -> Vec<SessionId> {
        let slots = self.slots.map.lock().await;
        let mut ids: Vec<SessionId> = slots
            .iter()
            .filter(|(_, slot)| slot.running)
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        ids
    }

    async fn stop(&self, session: SessionId) -> Result<(), ServiceError> {
        let generation = {
            let mut slots = self.slots.map.lock().await;
            let Some(slot) = slots.get_mut(&session) else {
                return Ok(());
            };
            if !slot.running {
                return Ok(());
            }
            tracing::info!(%session, "stopping the session actor");
            if let Some(cancel) = &slot.cancel {
                cancel.cancel();
            }
            slot.pending = None;
            slot.generation
        };

        loop {
            let notified = self.slots.idle.notified();
            tokio::pin!(notified);
            let still_this_actor = self
                .slots
                .map
                .lock()
                .await
                .get(&session)
                .is_some_and(|slot| slot.running && slot.generation == generation);
            if !still_this_actor {
                tracing::info!(%session, "session actor stopped");
                return Ok(());
            }
            notified.await;
        }
    }
}

async fn interrupt_if_running(
    slots: &Slots,
    session: SessionId,
    instruction: &str,
    images: Vec<robi_core::message::ImageAttachment>,
) -> bool {
    let mut slots = slots.map.lock().await;
    let Some(slot) = slots.get_mut(&session) else {
        return false;
    };
    if !slot.running {
        return false;
    }
    replace_pending(slot, session, instruction.to_owned(), images);
    true
}

fn replace_pending(
    slot: &mut Slot,
    session: SessionId,
    instruction: String,
    images: Vec<robi_core::message::ImageAttachment>,
) {
    tracing::info!(%session, "replaced the pending instruction");
    if let Some(cancel) = &slot.cancel {
        cancel.cancel();
    }
    slot.pending = Some(Work::Instruction {
        instruction,
        images,
    });
}

async fn decide_then_resume(
    agent: &Agent,
    session: SessionId,
    call: ToolCallId,
    reject: Option<String>,
    cancel: CancellationToken,
) -> TurnOutcome {
    let settled = match &reject {
        Some(reason) => agent.reject(session, call, reason).await,
        None => agent.approve(session, call).await,
    };
    if let Err(error) = settled {
        tracing::error!(%session, %call, %error, "failed to settle a tool call");
        return TurnOutcome::Failed(error);
    }
    agent.resume(session, cancel).await
}

async fn skill_loads(
    sessions: Option<&Arc<ChatSessionService>>,
    session: SessionId,
    instruction: &str,
) -> Vec<robi_core::message::SkillLoad> {
    let Some(sessions) = sessions else {
        return Vec::new();
    };
    let Ok(chat) = sessions.get_chat_session(session).await else {
        return Vec::new();
    };
    let Ok(Some(workspace)) = sessions.workspaces.get_workspace(chat.workspace_id).await else {
        return Vec::new();
    };
    let root = std::path::PathBuf::from(&workspace.root);
    let root = root.canonicalize().unwrap_or(root);
    let home = crate::adapters::settings::home_dir()
        .ok()
        .and_then(|dir| dir.parent().map(|parent| parent.to_path_buf()));
    let skills = crate::agent::skills::scan(home.as_deref(), Some(&root));
    crate::agent::skills::loads_for_text(instruction, &skills)
}

async fn run_actor(
    agent: Agent,
    model: Arc<dyn Model>,
    store: Arc<dyn MessageStore>,
    sessions: Option<Arc<ChatSessionService>>,
    bus: Option<Arc<EventBus>>,
    slots: Arc<Slots>,
    session: SessionId,
) {
    loop {
        let (work, cancel) = {
            let mut guard = slots.map.lock().await;
            let Some(slot) = guard.get_mut(&session) else {
                tracing::error!(%session, "session actor lost its slot");
                drop(guard);
                slots.idle.notify_waiters();
                return;
            };
            let Some(work) = slot.pending.take() else {
                slot.running = false;
                slot.cancel = None;
                tracing::info!(%session, "session actor idle");
                drop(guard);
                slots.idle.notify_waiters();
                return;
            };
            let cancel = CancellationToken::new();
            slot.cancel = Some(cancel.clone());
            (work, cancel)
        };

        let outcome = match work {
            Work::Instruction {
                instruction,
                images,
            } => {
                tracing::info!(%session, "session actor started a turn");
                let skills = skill_loads(sessions.as_ref(), session, &instruction).await;
                agent
                    .user_input_with_skills(session, &instruction, skills, images, cancel)
                    .await
            }
            Work::Decision { call, reject } => {
                tracing::info!(
                    %session,
                    %call,
                    approved = reject.is_none(),
                    "session actor resumed a turn"
                );
                decide_then_resume(&agent, session, call, reject, cancel).await
            }
        };
        match &outcome {
            TurnOutcome::Complete => {
                tracing::info!(%session, "chat turn completed");
            }
            TurnOutcome::Paused => {
                tracing::info!(%session, "chat turn paused for approval");
            }
            TurnOutcome::Cancelled => {
                tracing::info!(%session, "chat turn cancelled");
            }
            TurnOutcome::Failed(error) => {
                tracing::error!(%session, %error, "chat turn failed");
            }
        }
        let title_sessions = if matches!(outcome, TurnOutcome::Complete) {
            sessions.clone()
        } else {
            None
        };
        if let Some(sessions) = title_sessions {
            let model = Arc::clone(&model);
            let store = Arc::clone(&store);
            let bus = bus.clone();
            tokio::spawn(async move {
                title_completed_turn(session, model, store, sessions, bus).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use robi_core::config::LoopConfig;
    use robi_core::error::{ModelError, StoreError, ToolError};
    use robi_core::event::NopSink;
    use robi_core::ids::{MessageId, SessionId, WorkspaceId};
    use robi_core::message::{Message, Role, ToolCall};
    use robi_core::model::{Delta, Model, ModelStream};
    use robi_core::store::MessageStore;
    use robi_core::tool::{ApprovalDecision, Tool, ToolRegistry, ToolRun};
    use tokio::sync::{mpsc, Notify};
    use tokio_util::sync::CancellationToken;

    use super::{AgentFactory, SerializedChatRuntime};
    use crate::adapters::model_source::{FixedModelSource, ModelSource};
    use crate::domain::chat_message::runtime::{ChatRuntime, SubmitOutcome};
    use crate::domain::chat_session::{
        model::{AgentMode, ChatSession, CreateChatSessionCommand, ModeOverride, ModelConfig},
        repo::ChatSessionRepository,
        service::ChatSessionService,
    };
    use crate::domain::error::ServiceError;
    use crate::domain::events::{EventBus, SESSION_UPDATED};
    use crate::domain::settings::store::SettingsStore;
    use crate::domain::workspace::repo::AnyWorkspace;

    struct MemoryStore {
        sessions: Mutex<Vec<SessionId>>,
        messages: Mutex<std::collections::HashMap<SessionId, Vec<Message>>>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                sessions: Mutex::new(Vec::new()),
                messages: Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl MessageStore for MemoryStore {
        fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
            let id = SessionId::new();
            self.sessions.lock().expect("sessions").push(id);
            self.messages
                .lock()
                .expect("messages")
                .insert(id, Vec::new());
            id
        }

        fn has_session(&self, session: SessionId) -> bool {
            self.sessions.lock().expect("sessions").contains(&session)
        }

        async fn messages(&self, session: SessionId) -> Result<Vec<Message>, StoreError> {
            self.messages
                .lock()
                .expect("messages")
                .get(&session)
                .cloned()
                .ok_or(StoreError::SessionNotFound(session))
        }

        async fn message(
            &self,
            session: SessionId,
            id: MessageId,
        ) -> Result<Option<Message>, StoreError> {
            let messages = self.messages.lock().expect("messages");
            let transcript = messages
                .get(&session)
                .ok_or(StoreError::SessionNotFound(session))?;
            Ok(transcript.iter().find(|message| message.id == id).cloned())
        }

        async fn append(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
            let mut messages = self.messages.lock().expect("messages");
            let transcript = messages
                .get_mut(&session)
                .ok_or(StoreError::SessionNotFound(session))?;
            transcript.push(message);
            Ok(())
        }

        async fn update(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
            let mut messages = self.messages.lock().expect("messages");
            let transcript = messages
                .get_mut(&session)
                .ok_or(StoreError::SessionNotFound(session))?;
            if let Some(existing) = transcript.iter_mut().find(|stored| stored.id == message.id) {
                *existing = message;
            } else {
                transcript.push(message);
            }
            Ok(())
        }
    }

    struct GateModel {
        started: mpsc::UnboundedSender<usize>,
        release: Arc<Notify>,
        transcripts: Arc<Mutex<Vec<Vec<String>>>>,
        calls: AtomicUsize,
        in_flight: AtomicUsize,
        max_in_flight: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Model for GateModel {
        async fn generate(
            &self,
            _session: SessionId,
            transcript: &[Message],
            cancel: CancellationToken,
        ) -> Result<ModelStream, ModelError> {
            let users = transcript
                .iter()
                .filter(|message| message.role == Role::User)
                .map(|message| message.content.clone())
                .collect();
            self.transcripts.lock().expect("transcripts").push(users);

            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            let _guard = InFlight(&self.in_flight);
            let _ = self.started.send(call);

            if call == 1 {
                cancel.cancelled().await;
                let (_tx, rx) = mpsc::channel(1);
                return Ok(ModelStream::new(rx));
            }

            self.release.notified().await;
            let (tx, rx) = mpsc::channel(1);
            let _ = tx.send(Delta::Finished(Message::assistant("done"))).await;
            Ok(ModelStream::new(rx))
        }
    }

    struct InFlight<'a>(&'a AtomicUsize);

    impl Drop for InFlight<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn runtime(store: Arc<dyn MessageStore>, model: Arc<dyn Model>) -> SerializedChatRuntime {
        runtime_with(store, model, Arc::new(ToolRegistry::new()))
    }

    fn runtime_with(
        store: Arc<dyn MessageStore>,
        model: Arc<dyn Model>,
        tools: Arc<ToolRegistry>,
    ) -> SerializedChatRuntime {
        SerializedChatRuntime::new(AgentFactory {
            store,
            events: Arc::new(NopSink),
            models: Arc::new(FixedModelSource::new(model)),
            tools,
            config: LoopConfig::default(),
            sessions: None,
            file_changes: None,
            bus: None,
            search: idle_search(),
            index: None,
            lsp: None,
            settings: None,
            mcp: None,
            originals: None,
        })
    }

    #[tokio::test]
    async fn a_newer_instruction_cancels_the_in_flight_turn() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let release = Arc::new(Notify::new());
        let transcripts = Arc::new(Mutex::new(Vec::new()));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let runtime = runtime(
            store.clone(),
            Arc::new(GateModel {
                started: started_tx,
                release: Arc::clone(&release),
                transcripts: Arc::clone(&transcripts),
                calls: AtomicUsize::new(0),
                in_flight: AtomicUsize::new(0),
                max_in_flight: Arc::clone(&max_in_flight),
            }),
        );

        assert_eq!(
            runtime
                .submit(session, "first".into(), Vec::new())
                .await
                .unwrap(),
            SubmitOutcome::Accepted
        );
        let first = tokio::time::timeout(Duration::from_secs(5), started_rx.recv())
            .await
            .expect("first model call")
            .expect("started channel");
        assert_eq!(first, 1);

        assert_eq!(
            runtime
                .submit(session, "second".into(), Vec::new())
                .await
                .unwrap(),
            SubmitOutcome::Accepted
        );
        let second = tokio::time::timeout(Duration::from_secs(5), started_rx.recv())
            .await
            .expect("second model call")
            .expect("started channel");
        assert_eq!(second, 2);
        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(
            transcripts.lock().expect("transcripts").as_slice(),
            &[
                vec!["first".to_owned()],
                vec!["first".to_owned(), "second".to_owned()]
            ]
        );

        release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let messages = store.messages(session).await.unwrap();
                if messages.iter().any(|message| message.content == "done") {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("second turn finishes");
    }

    #[tokio::test]
    async fn stop_cancels_the_in_flight_turn_and_waits_until_the_actor_exits() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let transcripts = Arc::new(Mutex::new(Vec::new()));
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let runtime = runtime(
            store.clone(),
            Arc::new(GateModel {
                started: started_tx,
                release: Arc::new(Notify::new()),
                transcripts,
                calls: AtomicUsize::new(0),
                in_flight: AtomicUsize::new(0),
                max_in_flight: Arc::new(AtomicUsize::new(0)),
            }),
        );

        runtime
            .submit(session, "first".into(), Vec::new())
            .await
            .unwrap();
        let started = tokio::time::timeout(Duration::from_secs(5), started_rx.recv())
            .await
            .expect("model call")
            .expect("started channel");
        assert_eq!(started, 1);

        tokio::time::timeout(Duration::from_secs(5), runtime.stop(session))
            .await
            .expect("stop returns")
            .unwrap();
        assert!(runtime.running_session_ids().await.is_empty());

        let messages = store.messages(session).await.unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["first"]
        );

        runtime.stop(session).await.unwrap();
        assert!(runtime.running_session_ids().await.is_empty());
    }

    #[tokio::test]
    async fn a_pending_approval_is_rejected_and_the_instruction_is_appended() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        store
            .append(
                session,
                Message::assistant_with_tool_calls(
                    "run",
                    vec![ToolCall::new("shell", serde_json::json!({}))],
                ),
            )
            .await
            .unwrap();
        let before = store.messages(session).await.unwrap();
        let tools = ToolRegistry::new();
        tools
            .register(Arc::new(GateTool {
                name: "shell",
                decision: ApprovalDecision::NeedsApproval,
            }))
            .unwrap();
        let runtime = runtime_with(
            store.clone(),
            Arc::new(robi_core::model::UnavailableModel),
            Arc::new(tools),
        );

        assert_eq!(
            runtime
                .submit(session, "hello".into(), Vec::new())
                .await
                .unwrap(),
            SubmitOutcome::Accepted
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let messages = store.messages(session).await.unwrap();
                if messages.iter().any(|message| message.content == "hello") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the instruction was appended");
        let messages = store.messages(session).await.unwrap();
        assert!(messages.len() > before.len());
        let call = &messages[0].tool_calls[0];
        assert_eq!(
            call.approval_status,
            robi_core::message::ApprovalStatus::Rejected
        );
        assert!(call.execution_status.is_terminal());
        let hello = messages
            .iter()
            .position(|message| message.content == "hello")
            .unwrap();
        assert!(
            messages[..hello]
                .iter()
                .any(|message| message.role == robi_core::message::Role::Tool),
            "the rejection is stored before the new message"
        );
    }

    #[tokio::test]
    async fn an_unfinished_call_that_needs_no_approval_is_settled() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        store
            .append(
                session,
                Message::assistant_with_tool_calls(
                    "check",
                    vec![ToolCall::new("diagnostics", serde_json::json!({}))],
                ),
            )
            .await
            .unwrap();
        let tools = ToolRegistry::new();
        tools
            .register(Arc::new(GateTool {
                name: "diagnostics",
                decision: ApprovalDecision::AllowImmediately,
            }))
            .unwrap();
        let runtime = runtime_with(
            store.clone(),
            Arc::new(robi_core::model::UnavailableModel),
            Arc::new(tools),
        );

        assert_eq!(
            runtime
                .submit(session, "hello".into(), Vec::new())
                .await
                .unwrap(),
            SubmitOutcome::Accepted
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let messages = store.messages(session).await.unwrap();
                if messages.iter().any(|message| message.content == "hello") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the instruction was appended");
        let messages = store.messages(session).await.unwrap();
        let call = &messages[0].tool_calls[0];
        assert_eq!(
            call.execution_status,
            robi_core::message::ExecutionStatus::Succeeded
        );
    }

    struct GateTool {
        name: &'static str,
        decision: ApprovalDecision,
    }

    #[async_trait]
    impl Tool for GateTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "a test tool"
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({})
        }

        async fn requires_approval(&self, _args: &serde_json::Value) -> ApprovalDecision {
            self.decision
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
            _run: ToolRun,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"ok": true}))
        }
    }

    struct HoldModel {
        started: mpsc::UnboundedSender<SessionId>,
        release: CancellationToken,
    }

    #[async_trait]
    impl Model for HoldModel {
        async fn generate(
            &self,
            session: SessionId,
            _transcript: &[Message],
            _cancel: CancellationToken,
        ) -> Result<ModelStream, ModelError> {
            let _ = self.started.send(session);
            self.release.cancelled().await;
            let (tx, rx) = mpsc::channel(1);
            let _ = tx.send(Delta::Finished(Message::assistant("done"))).await;
            Ok(ModelStream::new(rx))
        }
    }

    #[tokio::test]
    async fn running_session_ids_are_the_sessions_with_a_live_actor() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let release = CancellationToken::new();
        let store = Arc::new(MemoryStore::new());
        let first = store.create_session(WorkspaceId::new());
        let second = store.create_session(WorkspaceId::new());
        let runtime = runtime(
            store,
            Arc::new(HoldModel {
                started: started_tx,
                release: release.clone(),
            }),
        );

        assert!(runtime.running_session_ids().await.is_empty());

        runtime
            .submit(first, "one".into(), Vec::new())
            .await
            .unwrap();
        let started = tokio::time::timeout(Duration::from_secs(5), started_rx.recv())
            .await
            .expect("first actor")
            .expect("started channel");
        assert_eq!(started, first);
        assert_eq!(runtime.running_session_ids().await, vec![first]);

        runtime
            .submit(second, "two".into(), Vec::new())
            .await
            .unwrap();
        let started = tokio::time::timeout(Duration::from_secs(5), started_rx.recv())
            .await
            .expect("second actor")
            .expect("started channel");
        assert_eq!(started, second);
        let mut running = runtime.running_session_ids().await;
        running.sort();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(running, expected);

        release.cancel();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if runtime.running_session_ids().await.is_empty() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("actors go idle");
    }

    struct FailSource;

    #[async_trait]
    impl ModelSource for FailSource {
        async fn model(
            &self,
            _tools: Arc<ToolRegistry>,
            _workspace: Option<std::path::PathBuf>,
            _mode: AgentMode,
            _choice: ModeOverride,
            _plan_path: Option<String>,
        ) -> Result<Arc<dyn Model>, ServiceError> {
            Err(ServiceError::BadRequest(
                "opencode_go_api_key is not set".into(),
            ))
        }
    }

    #[tokio::test]
    async fn a_model_that_cannot_be_built_does_not_mark_the_session_running() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let runtime = SerializedChatRuntime::new(AgentFactory {
            store,
            events: Arc::new(NopSink),
            models: Arc::new(FailSource),
            tools: Arc::new(ToolRegistry::new()),
            config: LoopConfig::default(),
            sessions: None,
            file_changes: None,
            bus: None,
            search: idle_search(),
            index: None,
            lsp: None,
            settings: None,
            mcp: None,
            originals: None,
        });

        let error = runtime
            .submit(session, "hello".into(), Vec::new())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("opencode_go_api_key is not set".into())
        );
        assert!(runtime.running_session_ids().await.is_empty());
    }

    struct TitleModel {
        calls: AtomicUsize,
        prompts: Mutex<Vec<String>>,
        fail_user_turns: bool,
    }

    #[async_trait]
    impl Model for TitleModel {
        async fn generate(
            &self,
            _session: SessionId,
            transcript: &[Message],
            _cancel: CancellationToken,
        ) -> Result<ModelStream, ModelError> {
            let prompt = transcript
                .iter()
                .map(|message| message.content.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let naming = prompt.contains("Name this conversation");
            self.prompts.lock().expect("prompts").push(prompt);
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (tx, rx) = mpsc::channel(1);
            if self.fail_user_turns && !naming {
                let _ = tx
                    .send(Delta::Failed(ModelError::Provider("nope".into())))
                    .await;
            } else {
                let text = if naming {
                    "\"Parser cleanup.\""
                } else {
                    "done"
                };
                let _ = tx.send(Delta::Finished(Message::assistant(text))).await;
            }
            Ok(ModelStream::new(rx))
        }
    }

    struct ChoiceSource {
        seen: Arc<Mutex<Option<(AgentMode, ModeOverride)>>>,
        model: Arc<dyn Model>,
    }

    #[async_trait]
    impl ModelSource for ChoiceSource {
        async fn model(
            &self,
            _tools: Arc<ToolRegistry>,
            _workspace: Option<std::path::PathBuf>,
            mode: AgentMode,
            choice: ModeOverride,
            _plan_path: Option<String>,
        ) -> Result<Arc<dyn Model>, ServiceError> {
            *self.seen.lock().expect("choice") = Some((mode, choice));
            Ok(Arc::clone(&self.model))
        }
    }

    #[tokio::test]
    async fn a_new_actor_is_built_with_the_session_model_config() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let choice = ModelConfig {
            agent: ModeOverride {
                model: Some("glm-5.2".into()),
                reasoning_effort: Some("high".into()),
            },
            ..ModelConfig::default()
        };
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(MemorySessions::with_model_config(session, choice.clone())),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            plan_cleaner: None,
        });
        let seen = Arc::new(Mutex::new(None));
        let runtime = SerializedChatRuntime::new(AgentFactory {
            store,
            events: Arc::new(NopSink),
            models: Arc::new(ChoiceSource {
                seen: Arc::clone(&seen),
                model: Arc::new(TitleModel {
                    calls: AtomicUsize::new(0),
                    prompts: Mutex::new(Vec::new()),
                    fail_user_turns: false,
                }),
            }),
            tools: Arc::new(ToolRegistry::new()),
            config: LoopConfig::default(),
            sessions: Some(sessions),
            file_changes: Some(Arc::new(
                crate::domain::file_change::memory::MemoryFileChangeRepository::new(),
            )),
            bus: None,
            search: idle_search(),
            index: None,
            lsp: None,
            settings: None,
            mcp: None,
            originals: None,
        });

        runtime
            .submit(session, "hello".into(), Vec::new())
            .await
            .unwrap();
        assert_eq!(
            seen.lock().expect("choice").clone(),
            Some((AgentMode::Agent, choice.agent.clone()))
        );
    }

    #[tokio::test]
    async fn lsp_off_keeps_the_language_server_tools_out_of_the_actor() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(MemorySessions::new(session)),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            plan_cleaner: None,
        });
        let settings = Arc::new(crate::domain::settings::memory::MemorySettingsStore::new());
        settings
            .set(
                crate::domain::settings::keys::LSP,
                crate::domain::settings::keys::LSP_OFF.into(),
                false,
            )
            .await
            .unwrap();
        let names = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&names);
        let runtime = SerializedChatRuntime::new(AgentFactory {
            store,
            events: Arc::new(NopSink),
            models: Arc::new(ToolListSource {
                names: seen,
                model: Arc::new(TitleModel {
                    calls: AtomicUsize::new(0),
                    prompts: Mutex::new(Vec::new()),
                    fail_user_turns: false,
                }),
            }),
            tools: Arc::new(ToolRegistry::new()),
            config: LoopConfig::default(),
            sessions: Some(sessions),
            file_changes: Some(Arc::new(
                crate::domain::file_change::memory::MemoryFileChangeRepository::new(),
            )),
            bus: None,
            search: idle_search(),
            index: None,
            lsp: None,
            settings: Some(settings),
            mcp: None,
            originals: None,
        });

        runtime
            .submit(session, "hello".into(), Vec::new())
            .await
            .unwrap();
        let names = names.lock().expect("names").clone();
        assert!(names.iter().any(|name| name == "read_file"));
        assert!(!names.iter().any(|name| name == "diagnostics"));
    }

    struct ToolListSource {
        names: Arc<Mutex<Vec<String>>>,
        model: Arc<dyn Model>,
    }

    #[async_trait]
    impl ModelSource for ToolListSource {
        async fn model(
            &self,
            tools: Arc<ToolRegistry>,
            _workspace: Option<std::path::PathBuf>,
            _mode: AgentMode,
            _choice: ModeOverride,
            _plan_path: Option<String>,
        ) -> Result<Arc<dyn Model>, ServiceError> {
            *self.names.lock().expect("names") = tools.names();
            Ok(Arc::clone(&self.model))
        }
    }

    fn titled_runtime(
        store: Arc<dyn MessageStore>,
        model: Arc<dyn Model>,
        sessions: Arc<ChatSessionService>,
        bus: Arc<EventBus>,
    ) -> SerializedChatRuntime {
        SerializedChatRuntime::new(AgentFactory {
            store,
            events: Arc::new(NopSink),
            models: Arc::new(FixedModelSource::new(model)),
            tools: Arc::new(ToolRegistry::new()),
            config: LoopConfig::default(),
            sessions: Some(sessions),
            file_changes: Some(Arc::new(
                crate::domain::file_change::memory::MemoryFileChangeRepository::new(),
            )),
            bus: Some(bus),
            search: idle_search(),
            index: None,
            lsp: None,
            settings: None,
            mcp: None,
            originals: None,
        })
    }

    fn idle_search() -> Arc<dyn crate::agent::web::SearchEngine> {
        Arc::new(IdleSearch)
    }

    struct IdleSearch;

    #[async_trait]
    impl crate::agent::web::SearchEngine for IdleSearch {
        async fn search(
            &self,
            _query: &str,
        ) -> Result<Vec<crate::agent::web::SearchHit>, crate::agent::web::SearchError> {
            Err(crate::agent::web::SearchError(
                "search is not configured".into(),
            ))
        }
    }

    #[tokio::test]
    async fn a_completed_turn_stores_a_title_when_the_session_is_unnamed() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(MemorySessions::new(session)),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            plan_cleaner: None,
        });
        let bus = Arc::new(EventBus::new());
        let mut subscription = bus.subscribe();
        let model = Arc::new(TitleModel {
            calls: AtomicUsize::new(0),
            prompts: Mutex::new(Vec::new()),
            fail_user_turns: false,
        });
        let runtime = titled_runtime(
            store.clone(),
            model.clone(),
            Arc::clone(&sessions),
            Arc::clone(&bus),
        );

        runtime
            .submit(session, "rename the parser".into(), Vec::new())
            .await
            .unwrap();

        let named = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(title) = sessions
                    .get_chat_session(session)
                    .await
                    .unwrap()
                    .title
                    .clone()
                {
                    return title;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("title is stored");
        assert_eq!(named, "Parser cleanup");

        let envelope = tokio::time::timeout(Duration::from_secs(5), subscription.recv())
            .await
            .expect("session_updated")
            .expect("envelope");
        assert_eq!(envelope.event_type, SESSION_UPDATED);
        assert_eq!(envelope.subject, session.to_string());
        assert_eq!(envelope.data["session_id"], session.to_string());

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if runtime.running_session_ids().await.is_empty() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("actor goes idle");

        let prompts = model.prompts.lock().expect("prompts");
        assert!(prompts
            .iter()
            .any(|prompt| prompt.contains("rename the parser")));
        assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_existing_title_is_not_replaced() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(MemorySessions::named(session, "Kept")),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            plan_cleaner: None,
        });
        let model = Arc::new(TitleModel {
            calls: AtomicUsize::new(0),
            prompts: Mutex::new(Vec::new()),
            fail_user_turns: false,
        });
        let runtime = titled_runtime(
            store,
            model.clone(),
            sessions.clone(),
            Arc::new(EventBus::new()),
        );

        runtime
            .submit(session, "hello".into(), Vec::new())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if runtime.running_session_ids().await.is_empty()
                    && model.calls.load(Ordering::SeqCst) >= 1
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("turn finishes");
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(model.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            sessions
                .get_chat_session(session)
                .await
                .unwrap()
                .title
                .as_deref(),
            Some("Kept")
        );
    }

    #[tokio::test]
    async fn a_failed_turn_does_not_name_the_session() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(MemorySessions::new(session)),
            workspaces: Arc::new(AnyWorkspace),
            events: None,
            plan_cleaner: None,
        });
        let model = Arc::new(TitleModel {
            calls: AtomicUsize::new(0),
            prompts: Mutex::new(Vec::new()),
            fail_user_turns: true,
        });
        let runtime = titled_runtime(
            store,
            model.clone(),
            sessions.clone(),
            Arc::new(EventBus::new()),
        );

        runtime
            .submit(session, "hello".into(), Vec::new())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if runtime.running_session_ids().await.is_empty() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("actor goes idle");
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(model.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            sessions.get_chat_session(session).await.unwrap().title,
            None
        );
    }

    /// Chat session rows for title tests. Message storage stays on `MemoryStore`.
    struct MemorySessions {
        sessions: Mutex<std::collections::HashMap<SessionId, ChatSession>>,
    }

    impl MemorySessions {
        fn new(id: SessionId) -> Self {
            Self::named_option(id, None)
        }

        fn named(id: SessionId, title: &str) -> Self {
            Self::named_option(id, Some(title.to_owned()))
        }

        fn with_model_config(id: SessionId, model_config: ModelConfig) -> Self {
            let sessions = Self::new(id);
            sessions
                .sessions
                .lock()
                .expect("sessions")
                .get_mut(&id)
                .expect("session")
                .model_config = model_config;
            sessions
        }

        fn named_option(id: SessionId, title: Option<String>) -> Self {
            let now = chrono::Utc::now();
            let session = ChatSession {
                id,
                workspace_id: WorkspaceId::new(),
                title,
                path_rules: crate::domain::chat_session::model::PathRules::default(),
                allow_hosts: Vec::new(),
                mcp_allows: Vec::new(),
                mode: AgentMode::Agent,
                model_config: crate::domain::chat_session::model::ModelConfig::default(),
                plan_path: None,
                created_at: now,
                updated_at: now,
                last_used_at: now,
            };
            let mut sessions = std::collections::HashMap::new();
            sessions.insert(id, session);
            Self {
                sessions: Mutex::new(sessions),
            }
        }
    }

    #[async_trait]
    impl ChatSessionRepository for MemorySessions {
        async fn create_chat_session(
            &self,
            _command: CreateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            Err(ServiceError::Unknown)
        }

        async fn get_chat_session(
            &self,
            id: SessionId,
        ) -> Result<Option<ChatSession>, ServiceError> {
            Ok(self.sessions.lock().expect("sessions").get(&id).cloned())
        }

        async fn list_chat_sessions(
            &self,
            _workspace_id: Option<WorkspaceId>,
        ) -> Result<Vec<ChatSession>, ServiceError> {
            Ok(Vec::new())
        }

        async fn update_chat_session(
            &self,
            command: crate::domain::chat_session::model::UpdateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            Err(ServiceError::NotFound(command.id.to_string()))
        }

        async fn set_plan_path(&self, id: SessionId, path: String) -> Result<(), ServiceError> {
            let mut sessions = self.sessions.lock().expect("sessions");
            let session = sessions
                .get_mut(&id)
                .ok_or_else(|| ServiceError::NotFound(id.to_string()))?;
            session.plan_path = Some(path);
            Ok(())
        }

        async fn set_title_if_unset(
            &self,
            id: SessionId,
            title: String,
        ) -> Result<Option<ChatSession>, ServiceError> {
            let mut sessions = self.sessions.lock().expect("sessions");
            let session = sessions
                .get_mut(&id)
                .ok_or_else(|| ServiceError::NotFound(id.to_string()))?;
            if session.title.is_some() {
                return Ok(None);
            }
            session.title = Some(title);
            session.updated_at = chrono::Utc::now();
            Ok(Some(session.clone()))
        }

        async fn delete_chat_session(&self, id: SessionId) -> Result<(), ServiceError> {
            Err(ServiceError::NotFound(id.to_string()))
        }
    }
}
