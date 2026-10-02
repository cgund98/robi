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
use robi_core::ids::SessionId;
use robi_core::message::{unresolved_turn, Message};
use robi_core::model::Model;
use robi_core::store::MessageStore;
use robi_core::tool::ToolRegistry;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::domain::{
    chat_message::runtime::{ChatRuntime, SubmitOutcome},
    error::ServiceError,
};

/// Process-wide pieces a session actor needs. The agent is not one of them.
pub struct AgentFactory {
    pub store: Arc<dyn MessageStore>,
    pub events: Arc<dyn EventSink>,
    pub model: Arc<dyn Model>,
    pub tools: Arc<ToolRegistry>,
    pub config: LoopConfig,
}

impl AgentFactory {
    fn build(&self) -> Agent {
        Agent::new(
            Arc::clone(&self.store),
            Arc::clone(&self.events),
            Arc::clone(&self.model),
            Arc::clone(&self.tools),
            self.config,
        )
    }
}

#[derive(Default)]
struct Slot {
    cancel: Option<CancellationToken>,
    pending: Option<String>,
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

    fn spawn_actor(&self, session: SessionId) {
        let agent = self.factory.build();
        let slots = Arc::clone(&self.slots);
        tokio::spawn(async move {
            run_actor(agent, slots, session).await;
        });
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

        let mut slots = self.slots.lock().await;
        let slot = slots.entry(session).or_default();
        if slot.running {
            replace_pending(slot, instruction);
            return Ok(SubmitOutcome::Accepted);
        }
        slot.running = true;
        slot.pending = Some(instruction);
        drop(slots);
        self.spawn_actor(session);
        Ok(SubmitOutcome::Accepted)
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
    slot.pending = Some(instruction);
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

fn map_store(error: StoreError) -> ServiceError {
    match error {
        StoreError::SessionNotFound(id) => ServiceError::NotFound(id.to_string()),
        StoreError::Backend(message) => {
            tracing::error!(error = %message, "chat message store failed");
            ServiceError::Unknown
        }
    }
}

async fn run_actor(agent: Agent, slots: Arc<Mutex<HashMap<SessionId, Slot>>>, session: SessionId) {
    loop {
        let (instruction, cancel) = {
            let mut guard = slots.lock().await;
            let Some(slot) = guard.get_mut(&session) else {
                return;
            };
            let Some(instruction) = slot.pending.take() else {
                slot.running = false;
                slot.cancel = None;
                return;
            };
            let cancel = CancellationToken::new();
            slot.cancel = Some(cancel.clone());
            (instruction, cancel)
        };

        let outcome = agent.user_input(session, &instruction, cancel).await;
        match outcome {
            TurnOutcome::Failed(error) => {
                tracing::warn!(%session, %error, "chat turn failed");
            }
            other => {
                tracing::debug!(%session, ?other, "chat turn finished");
            }
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
    use robi_core::ids::{SessionId, WorkspaceId};
    use robi_core::message::{Message, Role, ToolCall};
    use robi_core::model::{Delta, Model, ModelStream};
    use robi_core::store::MessageStore;
    use robi_core::tool::ToolRegistry;
    use tokio::sync::{mpsc, Notify};
    use tokio_util::sync::CancellationToken;

    use super::{AgentFactory, SerializedChatRuntime};
    use crate::domain::chat_message::runtime::{ChatRuntime, SubmitOutcome};

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
            model,
            tools: Arc::new(ToolRegistry::new()),
            config: LoopConfig::default(),
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
}
