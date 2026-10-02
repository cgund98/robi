use std::sync::Arc;

use robi_core::error::StoreError;
use robi_core::ids::{MessageId, SessionId, ToolCallId};
use robi_core::message::Message;
use robi_core::store::MessageStore;

use crate::domain::{
    chat_message::runtime::{ChatRuntime, SubmitOutcome},
    chat_session::service::ChatSessionService,
    error::ServiceError,
};

/// Accepts an instruction and reads the persisted transcript.
///
/// The session list and the transcript are different ports over the same
/// database. This service checks the session row, then either hands the
/// instruction to the runtime or reads `MessageStore` directly.
pub struct ChatMessageService {
    pub sessions: Arc<ChatSessionService>,
    pub runtime: Arc<dyn ChatRuntime>,
    pub store: Arc<dyn MessageStore>,
}

impl ChatMessageService {
    pub async fn submit_instruction(
        &self,
        session: SessionId,
        instruction: &str,
    ) -> Result<SubmitOutcome, ServiceError> {
        if instruction.trim().is_empty() {
            return Err(ServiceError::BadRequest(
                "instruction must not be empty".into(),
            ));
        }
        self.sessions.get_chat_session(session).await?;
        self.runtime.submit(session, instruction.to_owned()).await
    }

    /// Approve or reject one paused call and resume that turn.
    pub async fn decide_tool_call(
        &self,
        session: SessionId,
        call: ToolCallId,
        decision: &str,
        reason: Option<String>,
    ) -> Result<(), ServiceError> {
        self.sessions.get_chat_session(session).await?;
        let reject = match decision {
            "approve" => None,
            "reject" => Some(
                reason
                    .filter(|reason| !reason.trim().is_empty())
                    .unwrap_or_else(|| "rejected by the user".to_owned()),
            ),
            _ => {
                return Err(ServiceError::BadRequest(
                    "decision must be approve or reject".into(),
                ));
            }
        };
        self.runtime.decide(session, call, reject).await
    }

    pub async fn list_messages(&self, session: SessionId) -> Result<Vec<Message>, ServiceError> {
        self.sessions.get_chat_session(session).await?;
        self.store.messages(session).await.map_err(map_store)
    }

    pub async fn get_message(
        &self,
        session: SessionId,
        message_id: MessageId,
    ) -> Result<Message, ServiceError> {
        self.sessions.get_chat_session(session).await?;
        self.store
            .message(session, message_id)
            .await
            .map_err(map_store)?
            .ok_or_else(|| ServiceError::NotFound(message_id.to_string()))
    }

    /// Sessions that currently have a running actor. Read from memory.
    pub async fn running_session_ids(&self) -> Vec<SessionId> {
        self.runtime.running_session_ids().await
    }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::Utc;
    use robi_core::error::StoreError;
    use robi_core::ids::{MessageId, SessionId, WorkspaceId};
    use robi_core::message::Message;
    use robi_core::store::MessageStore;

    use super::*;
    use crate::domain::{
        chat_message::runtime::{ChatRuntime, SubmitOutcome},
        chat_session::{
            model::{ChatSession, CreateChatSessionCommand, UpdateChatSessionCommand},
            repo::ChatSessionRepository,
            service::ChatSessionService,
        },
        error::ServiceError,
    };

    struct FakeSessions {
        sessions: Mutex<HashMap<SessionId, ChatSession>>,
    }

    impl FakeSessions {
        fn new() -> Self {
            Self {
                sessions: Mutex::new(HashMap::new()),
            }
        }

        fn insert(&self, session: SessionId) {
            let now = Utc::now();
            self.sessions.lock().expect("fake sessions").insert(
                session,
                ChatSession {
                    id: session,
                    workspace_id: WorkspaceId::new(),
                    title: None,
                    path_rules: crate::domain::chat_session::model::PathRules::default(),
                    model_config: crate::domain::chat_session::model::ModelConfig::default(),
                    created_at: now,
                    updated_at: now,
                    last_used_at: now,
                },
            );
        }
    }

    #[async_trait]
    impl ChatSessionRepository for FakeSessions {
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
            Ok(self
                .sessions
                .lock()
                .expect("fake sessions")
                .get(&id)
                .cloned())
        }

        async fn list_chat_sessions(
            &self,
            _workspace_id: Option<WorkspaceId>,
        ) -> Result<Vec<ChatSession>, ServiceError> {
            Ok(Vec::new())
        }

        async fn update_chat_session(
            &self,
            _command: UpdateChatSessionCommand,
        ) -> Result<ChatSession, ServiceError> {
            Err(ServiceError::Unknown)
        }

        async fn set_title_if_unset(
            &self,
            _id: SessionId,
            _title: String,
        ) -> Result<Option<ChatSession>, ServiceError> {
            Err(ServiceError::Unknown)
        }

        async fn delete_chat_session(&self, _id: SessionId) -> Result<(), ServiceError> {
            Err(ServiceError::Unknown)
        }
    }

    struct FakeRuntime {
        seen: Mutex<Vec<(SessionId, String)>>,
        running: Mutex<Vec<SessionId>>,
    }

    impl FakeRuntime {
        fn new() -> Self {
            Self {
                seen: Mutex::new(Vec::new()),
                running: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ChatRuntime for FakeRuntime {
        async fn submit(
            &self,
            session: SessionId,
            instruction: String,
        ) -> Result<SubmitOutcome, ServiceError> {
            self.seen
                .lock()
                .expect("fake runtime")
                .push((session, instruction));
            Ok(SubmitOutcome::Accepted)
        }

        async fn running_session_ids(&self) -> Vec<SessionId> {
            self.running.lock().expect("fake runtime").clone()
        }

        async fn decide(
            &self,
            _session: SessionId,
            _call: ToolCallId,
            _reject: Option<String>,
        ) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    struct FakeStore {
        messages: Mutex<HashMap<SessionId, Vec<Message>>>,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                messages: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl MessageStore for FakeStore {
        fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
            SessionId::new()
        }

        fn has_session(&self, session: SessionId) -> bool {
            self.messages
                .lock()
                .expect("fake store")
                .contains_key(&session)
        }

        async fn messages(&self, session: SessionId) -> Result<Vec<Message>, StoreError> {
            self.messages
                .lock()
                .expect("fake store")
                .get(&session)
                .cloned()
                .ok_or(StoreError::SessionNotFound(session))
        }

        async fn message(
            &self,
            session: SessionId,
            id: MessageId,
        ) -> Result<Option<Message>, StoreError> {
            let messages = self.messages.lock().expect("fake store");
            let transcript = messages
                .get(&session)
                .ok_or(StoreError::SessionNotFound(session))?;
            Ok(transcript.iter().find(|message| message.id == id).cloned())
        }

        async fn append(&self, _session: SessionId, _message: Message) -> Result<(), StoreError> {
            Ok(())
        }

        async fn update(&self, _session: SessionId, _message: Message) -> Result<(), StoreError> {
            Ok(())
        }
    }

    fn service() -> (
        ChatMessageService,
        Arc<FakeSessions>,
        Arc<FakeRuntime>,
        Arc<FakeStore>,
    ) {
        let sessions = Arc::new(FakeSessions::new());
        let runtime = Arc::new(FakeRuntime::new());
        let store = Arc::new(FakeStore::new());
        let service = ChatMessageService {
            sessions: Arc::new(ChatSessionService {
                repository: sessions.clone(),
                workspaces: Arc::new(crate::domain::workspace::repo::AnyWorkspace),
            }),
            runtime: runtime.clone(),
            store: store.clone(),
        };
        (service, sessions, runtime, store)
    }

    #[tokio::test]
    async fn a_blank_instruction_is_rejected_before_the_runtime() {
        let (service, sessions, runtime, _) = service();
        let session = SessionId::new();
        sessions.insert(session);

        let error = service
            .submit_instruction(session, "   ")
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("instruction must not be empty".into())
        );
        assert!(runtime.seen.lock().expect("fake runtime").is_empty());
    }

    #[tokio::test]
    async fn a_missing_session_is_not_found() {
        let (service, _, runtime, _) = service();
        let session = SessionId::new();

        assert_eq!(
            service
                .submit_instruction(session, "hello")
                .await
                .unwrap_err(),
            ServiceError::NotFound(session.to_string())
        );
        assert_eq!(
            service.list_messages(session).await.unwrap_err(),
            ServiceError::NotFound(session.to_string())
        );
        assert!(runtime.seen.lock().expect("fake runtime").is_empty());
    }

    #[tokio::test]
    async fn a_real_instruction_is_handed_to_the_runtime() {
        let (service, sessions, runtime, store) = service();
        let session = SessionId::new();
        sessions.insert(session);
        store
            .messages
            .lock()
            .expect("fake store")
            .insert(session, vec![Message::user("earlier")]);

        assert_eq!(
            service
                .submit_instruction(session, "do the thing")
                .await
                .unwrap(),
            SubmitOutcome::Accepted
        );
        assert_eq!(
            runtime.seen.lock().expect("fake runtime").as_slice(),
            &[(session, "do the thing".to_owned())]
        );

        let listed = service.list_messages(session).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].content, "earlier");
    }

    #[tokio::test]
    async fn get_message_returns_the_matching_row() {
        let (service, sessions, _, store) = service();
        let session = SessionId::new();
        sessions.insert(session);
        let earlier = Message::user("earlier");
        let later = Message::user("later");
        store
            .messages
            .lock()
            .expect("fake store")
            .insert(session, vec![earlier.clone(), later.clone()]);

        let found = service.get_message(session, earlier.id).await.unwrap();
        assert_eq!(found, earlier);

        let missing = MessageId::new();
        assert_eq!(
            service.get_message(session, missing).await.unwrap_err(),
            ServiceError::NotFound(missing.to_string())
        );
    }

    #[tokio::test]
    async fn get_message_on_a_missing_session_is_not_found() {
        let (service, _, _, _) = service();
        let session = SessionId::new();

        assert_eq!(
            service
                .get_message(session, MessageId::new())
                .await
                .unwrap_err(),
            ServiceError::NotFound(session.to_string())
        );
    }

    #[tokio::test]
    async fn running_sessions_come_from_the_runtime() {
        let (service, _, runtime, _) = service();
        let session = SessionId::new();
        assert!(service.running_session_ids().await.is_empty());

        runtime.running.lock().expect("fake runtime").push(session);
        assert_eq!(service.running_session_ids().await, vec![session]);
    }
}
