//! A transcript that lives only for one child agent.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use robi_core::error::StoreError;
use robi_core::ids::{MessageId, SessionId, WorkspaceId};
use robi_core::message::Message;
use robi_core::store::MessageStore;

/// The child's transcript. It is dropped when the child returns, so file bodies
/// never land in the parent session.
#[derive(Default)]
pub(crate) struct MemoryStore {
    sessions: Mutex<HashMap<SessionId, Vec<Message>>>,
}

impl MemoryStore {
    fn with_session<T>(
        &self,
        session: SessionId,
        f: impl FnOnce(&mut Vec<Message>) -> T,
    ) -> Result<T, StoreError> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|err| err.into_inner());
        let messages = sessions
            .get_mut(&session)
            .ok_or(StoreError::SessionNotFound(session))?;
        Ok(f(messages))
    }
}

#[async_trait]
impl MessageStore for MemoryStore {
    fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
        let session = SessionId::new();
        self.sessions
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(session, Vec::new());
        session
    }

    fn has_session(&self, session: SessionId) -> bool {
        self.sessions
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .contains_key(&session)
    }

    async fn messages(&self, session: SessionId) -> Result<Vec<Message>, StoreError> {
        self.with_session(session, |messages| messages.clone())
    }

    async fn message(
        &self,
        session: SessionId,
        id: MessageId,
    ) -> Result<Option<Message>, StoreError> {
        self.with_session(session, |messages| {
            messages.iter().find(|message| message.id == id).cloned()
        })
    }

    async fn append(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        self.with_session(session, |messages| messages.push(message))
    }

    async fn update(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        self.with_session(session, |messages| {
            if let Some(slot) = messages.iter_mut().find(|stored| stored.id == message.id) {
                *slot = message;
            } else {
                messages.push(message);
            }
        })
    }
}
