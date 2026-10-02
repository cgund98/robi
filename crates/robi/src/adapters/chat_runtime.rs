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
use robi_core::error::{StoreError, TurnOutcome};
use robi_core::event::EventSink;
use robi_core::ids::{SessionId, ToolCallId};
use robi_core::message::{unresolved_turn, Message};
use robi_core::model::Model;
use robi_core::store::MessageStore;
use robi_core::tool::ToolRegistry;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    adapters::{model_source::ModelSource, session_title::title_completed_turn},
    domain::{
        chat_message::runtime::{ChatRuntime, SubmitOutcome},
        chat_session::service::ChatSessionService,
        error::ServiceError,
        events::EventFanOut,
        file_change::repo::FileChangeRepository,
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
    /// Publishes `session_updated` after a title is stored. Absent when there is
    /// no fan-out.
    pub fanout: Option<Arc<EventFanOut>>,
}

impl AgentFactory {
    fn build(&self, model: Arc<dyn Model>, tools: Arc<ToolRegistry>) -> Agent {
        Agent::new(
            Arc::clone(&self.store),
            Arc::clone(&self.events),
            model,
            tools,
            self.config,
        )
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
            crate::domain::chat_session::model::ModelConfig,
        ),
        ServiceError,
    > {
        let Some(sessions) = &self.sessions else {
            return Ok((
                Arc::clone(&self.tools),
                None,
                crate::domain::chat_session::model::ModelConfig::default(),
            ));
        };
        let chat = sessions.get_chat_session(session).await?;
        let choice = chat.model_config.clone();
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
        let ctx = Arc::new(crate::tools::ToolContext {
            session_id: session,
            root: root.clone(),
            sessions: Arc::clone(sessions),
            file_changes,
        });
        crate::tools::register_read_tools(&registry, Arc::clone(&ctx)).map_err(|err| {
            tracing::error!(%err, "failed to register read tools");
            ServiceError::Unknown
        })?;
        crate::tools::register_edit_tools(&registry, Arc::clone(&ctx)).map_err(|err| {
            tracing::error!(%err, "failed to register edit tools");
            ServiceError::Unknown
        })?;
        crate::tools::register_shell_tool(&registry, ctx).map_err(|err| {
            tracing::error!(%err, "failed to register the shell tool");
            ServiceError::Unknown
        })?;
        Ok((Arc::new(registry), Some(root), choice))
    }
}

enum Work {
    Instruction(String),
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
}

/// Serializes `user_input` for each chat session.
pub struct SerializedChatRuntime {
    factory: AgentFactory,
    slots: Arc<Mutex<HashMap<SessionId, Slot>>>,
}

impl SerializedChatRuntime {
    pub fn new(factory: AgentFactory) -> Self {
        Self {
            factory,
            slots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn spawn_actor(&self, session: SessionId, model: Arc<dyn Model>, tools: Arc<ToolRegistry>) {
        let agent = self.factory.build(Arc::clone(&model), tools);
        let slots = Arc::clone(&self.slots);
        let store = Arc::clone(&self.factory.store);
        let sessions = self.factory.sessions.clone();
        let fanout = self.factory.fanout.clone();
        tokio::spawn(async move {
            run_actor(agent, model, store, sessions, fanout, slots, session).await;
        });
    }

    async fn start_actor(
        &self,
        session: SessionId,
        instruction: String,
        model: Arc<dyn Model>,
        tools: Arc<ToolRegistry>,
    ) -> Result<SubmitOutcome, ServiceError> {
        let mut slots = self.slots.lock().await;
        let slot = slots.entry(session).or_default();
        if slot.running {
            replace_pending(slot, instruction);
            return Ok(SubmitOutcome::Accepted);
        }
        slot.running = true;
        slot.pending = Some(Work::Instruction(instruction));
        drop(slots);
        self.spawn_actor(session, model, tools);
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
        let mut slots = self.slots.lock().await;
        let slot = slots.entry(session).or_default();
        if slot.running {
            return Err(ServiceError::Conflict("chat session is running".into()));
        }
        slot.running = true;
        slot.pending = Some(Work::Decision { call, reject });
        drop(slots);
        self.spawn_actor(session, model, tools);
        Ok(())
    }

    async fn actor_running(&self, session: SessionId) -> bool {
        self.slots
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
    ) -> Result<SubmitOutcome, ServiceError> {
        if interrupt_if_running(&self.slots, session, &instruction).await {
            return Ok(SubmitOutcome::Accepted);
        }

        if awaiting_approval(&self.factory.store, session).await? {
            if interrupt_if_running(&self.slots, session, &instruction).await {
                return Ok(SubmitOutcome::Accepted);
            }
            return Ok(SubmitOutcome::AwaitingApproval);
        }

        if self.actor_running(session).await {
            let mut slots = self.slots.lock().await;
            let slot = slots.entry(session).or_default();
            if slot.running {
                replace_pending(slot, instruction);
                return Ok(SubmitOutcome::Accepted);
            }
        }

        // Resolve before the slot is marked running, so a missing key does not
        // leave an actor that will never start.
        let (tools, workspace, choice) = self.factory.session_registry(session).await?;
        let model = self
            .factory
            .models
            .model(Arc::clone(&tools), workspace, choice)
            .await?;
        self.start_actor(session, instruction, model, tools).await
    }

    async fn decide(
        &self,
        session: SessionId,
        call: ToolCallId,
        reject: Option<String>,
    ) -> Result<(), ServiceError> {
        if self.actor_running(session).await {
            return Err(ServiceError::Conflict("chat session is running".into()));
        }
        let (tools, workspace, choice) = self.factory.session_registry(session).await?;
        let model = self
            .factory
            .models
            .model(Arc::clone(&tools), workspace, choice)
            .await?;
        self.start_decision(session, call, reject, model, tools)
            .await
    }

    async fn running_session_ids(&self) -> Vec<SessionId> {
        let slots = self.slots.lock().await;
        let mut ids: Vec<SessionId> = slots
            .iter()
            .filter(|(_, slot)| slot.running)
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        ids
    }
}

async fn interrupt_if_running(
    slots: &Mutex<HashMap<SessionId, Slot>>,
    session: SessionId,
    instruction: &str,
) -> bool {
    let mut slots = slots.lock().await;
    let Some(slot) = slots.get_mut(&session) else {
        return false;
    };
    if !slot.running {
        return false;
    }
    replace_pending(slot, instruction.to_owned());
    true
}

fn replace_pending(slot: &mut Slot, instruction: String) {
    if let Some(cancel) = &slot.cancel {
        cancel.cancel();
    }
    slot.pending = Some(Work::Instruction(instruction));
}

async fn awaiting_approval(
    store: &Arc<dyn MessageStore>,
    session: SessionId,
) -> Result<bool, ServiceError> {
    let messages = store.messages(session).await.map_err(map_store)?;
    Ok(transcript_awaits_approval(&messages))
}

fn transcript_awaits_approval(messages: &[Message]) -> bool {
    unresolved_turn(messages)
        .is_some_and(|index| messages[index].pending_approval_calls().next().is_some())
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
        return TurnOutcome::Failed(error);
    }
    agent.resume(session, cancel).await
}

fn map_store(error: StoreError) -> ServiceError {
    match error {
        StoreError::SessionNotFound(id) => ServiceError::NotFound(id.to_string()),
        StoreError::Backend(message) => {
            tracing::error!(error = %message, "chat message store failed");
            ServiceError::Unknown
        }
    }
}

async fn run_actor(
    agent: Agent,
    model: Arc<dyn Model>,
    store: Arc<dyn MessageStore>,
    sessions: Option<Arc<ChatSessionService>>,
    fanout: Option<Arc<EventFanOut>>,
    slots: Arc<Mutex<HashMap<SessionId, Slot>>>,
    session: SessionId,
) {
    loop {
        let (work, cancel) = {
            let mut guard = slots.lock().await;
            let Some(slot) = guard.get_mut(&session) else {
                return;
            };
            let Some(work) = slot.pending.take() else {
                slot.running = false;
                slot.cancel = None;
                return;
            };
            let cancel = CancellationToken::new();
            slot.cancel = Some(cancel.clone());
            (work, cancel)
        };

        let outcome = match work {
            Work::Instruction(instruction) => agent.user_input(session, &instruction, cancel).await,
            Work::Decision { call, reject } => {
                decide_then_resume(&agent, session, call, reject, cancel).await
            }
        };
        match &outcome {
            TurnOutcome::Failed(error) => {
                tracing::warn!(%session, %error, "chat turn failed");
            }
            other => {
                tracing::debug!(%session, ?other, "chat turn finished");
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
            let fanout = fanout.clone();
            tokio::spawn(async move {
                title_completed_turn(session, model, store, sessions, fanout).await;
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
    use robi_core::error::{ModelError, StoreError};
    use robi_core::event::NopSink;
    use robi_core::ids::{MessageId, SessionId, WorkspaceId};
    use robi_core::message::{Message, Role, ToolCall};
    use robi_core::model::{Delta, Model, ModelStream};
    use robi_core::store::MessageStore;
    use robi_core::tool::ToolRegistry;
    use tokio::sync::{mpsc, Notify};
    use tokio_util::sync::CancellationToken;

    use super::{AgentFactory, SerializedChatRuntime};
    use crate::adapters::model_source::{FixedModelSource, ModelSource};
    use crate::domain::chat_message::runtime::{ChatRuntime, SubmitOutcome};
    use crate::domain::chat_session::{
        model::{ChatSession, CreateChatSessionCommand, ModelConfig},
        repo::ChatSessionRepository,
        service::ChatSessionService,
    };
    use crate::domain::error::ServiceError;
    use crate::domain::events::{EventFanOut, SESSION_UPDATED};
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
        SerializedChatRuntime::new(AgentFactory {
            store,
            events: Arc::new(NopSink),
            models: Arc::new(FixedModelSource::new(model)),
            tools: Arc::new(ToolRegistry::new()),
            config: LoopConfig::default(),
            sessions: None,
            file_changes: None,
            fanout: None,
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
            runtime.submit(session, "first".into()).await.unwrap(),
            SubmitOutcome::Accepted
        );
        let first = tokio::time::timeout(Duration::from_secs(5), started_rx.recv())
            .await
            .expect("first model call")
            .expect("started channel");
        assert_eq!(first, 1);

        assert_eq!(
            runtime.submit(session, "second".into()).await.unwrap(),
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
    async fn a_pending_approval_is_not_appended() {
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
        let runtime = runtime(store.clone(), Arc::new(robi_core::model::UnavailableModel));

        assert_eq!(
            runtime.submit(session, "hello".into()).await.unwrap(),
            SubmitOutcome::AwaitingApproval
        );
        assert_eq!(store.messages(session).await.unwrap(), before);
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

        runtime.submit(first, "one".into()).await.unwrap();
        let started = tokio::time::timeout(Duration::from_secs(5), started_rx.recv())
            .await
            .expect("first actor")
            .expect("started channel");
        assert_eq!(started, first);
        assert_eq!(runtime.running_session_ids().await, vec![first]);

        runtime.submit(second, "two".into()).await.unwrap();
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
            _choice: crate::domain::chat_session::model::ModelConfig,
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
            fanout: None,
        });

        let error = runtime.submit(session, "hello".into()).await.unwrap_err();
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
        seen: Arc<Mutex<Option<ModelConfig>>>,
        model: Arc<dyn Model>,
    }

    #[async_trait]
    impl ModelSource for ChoiceSource {
        async fn model(
            &self,
            _tools: Arc<ToolRegistry>,
            _workspace: Option<std::path::PathBuf>,
            choice: ModelConfig,
        ) -> Result<Arc<dyn Model>, ServiceError> {
            *self.seen.lock().expect("choice") = Some(choice);
            Ok(Arc::clone(&self.model))
        }
    }

    #[tokio::test]
    async fn a_new_actor_is_built_with_the_session_model_config() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let choice = ModelConfig {
            model: Some("glm-5.2".into()),
            reasoning_effort: Some("high".into()),
        };
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(MemorySessions::with_model_config(session, choice.clone())),
            workspaces: Arc::new(AnyWorkspace),
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
            fanout: None,
        });

        runtime.submit(session, "hello".into()).await.unwrap();
        assert_eq!(seen.lock().expect("choice").clone(), Some(choice));
    }

    fn titled_runtime(
        store: Arc<dyn MessageStore>,
        model: Arc<dyn Model>,
        sessions: Arc<ChatSessionService>,
        fanout: Arc<EventFanOut>,
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
            fanout: Some(fanout),
        })
    }

    #[tokio::test]
    async fn a_completed_turn_stores_a_title_when_the_session_is_unnamed() {
        let store = Arc::new(MemoryStore::new());
        let session = store.create_session(WorkspaceId::new());
        let sessions = Arc::new(ChatSessionService {
            repository: Arc::new(MemorySessions::new(session)),
            workspaces: Arc::new(AnyWorkspace),
        });
        let fanout = Arc::new(EventFanOut::new());
        let mut subscription = fanout.subscribe();
        let model = Arc::new(TitleModel {
            calls: AtomicUsize::new(0),
            prompts: Mutex::new(Vec::new()),
            fail_user_turns: false,
        });
        let runtime = titled_runtime(
            store.clone(),
            model.clone(),
            Arc::clone(&sessions),
            Arc::clone(&fanout),
        );

        runtime
            .submit(session, "rename the parser".into())
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
        assert_eq!(envelope.data["title"], "Parser cleanup");

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
            Arc::new(EventFanOut::new()),
        );

        runtime.submit(session, "hello".into()).await.unwrap();
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
            Arc::new(EventFanOut::new()),
        );

        runtime.submit(session, "hello".into()).await.unwrap();
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
                model_config: crate::domain::chat_session::model::ModelConfig::default(),
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
