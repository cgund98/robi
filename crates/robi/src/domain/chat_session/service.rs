use std::sync::Arc;

use robi_core::ids::SessionId;

use crate::domain::{
    chat_session::{
        model::{ChatSession, CreateChatSessionCommand, UpdateChatSessionCommand},
        repo::ChatSessionRepository,
    },
    error::ServiceError,
};

/// A chat session title longer than this is rejected before it is written.
pub const CHAT_SESSION_TITLE_MAX_CHARS: usize = 200;

pub struct ChatSessionService {
    pub repository: Arc<dyn ChatSessionRepository>,
}

impl ChatSessionService {
    pub async fn create_chat_session(
        &self,
        command: CreateChatSessionCommand,
    ) -> Result<ChatSession, ServiceError> {
        let title = normalize_new_title(command.title)?;
        self.repository
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: command.workspace_id,
                title,
            })
            .await
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
        command: UpdateChatSessionCommand,
    ) -> Result<ChatSession, ServiceError> {
        validate_title(&command.title)?;
        self.repository.update_chat_session(command).await
    }

    pub async fn delete_chat_session(&self, id: SessionId) -> Result<(), ServiceError> {
        self.repository.delete_chat_session(id).await
    }
}

/// `None` and `""` both mean "not named yet", so the model can title the chat session
/// after the first turn.
fn normalize_new_title(title: Option<String>) -> Result<Option<String>, ServiceError> {
    match title {
        Some(title) if !title.is_empty() => {
            validate_title(&title)?;
            Ok(Some(title))
        }
        _ => Ok(None),
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
    use crate::domain::chat_session::model::UpdateChatSessionCommand;

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
            session.title = Some(command.title);
            session.updated_at = Utc::now();
            Ok(session.clone())
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
        }
    }

    #[tokio::test]
    async fn create_chat_session_stores_the_title() {
        let service = service();
        let workspace_id = WorkspaceId::new();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: Some("Notes".into()),
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
            })
            .await
            .unwrap();
        service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: drop,
                title: Some("drop".into()),
            })
            .await
            .unwrap();

        let listed = service.list_chat_sessions(Some(keep)).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].workspace_id, keep);
        assert_eq!(listed[0].title.as_deref(), Some("keep"));
    }

    #[tokio::test]
    async fn update_chat_session_is_not_found_when_missing() {
        let service = service();
        let id = SessionId::new();
        let error = service
            .update_chat_session(UpdateChatSessionCommand {
                id,
                title: "Renamed".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(error, ServiceError::NotFound(id.to_string()));
    }

    #[tokio::test]
    async fn delete_chat_session_removes_it_and_missing_is_not_found() {
        let service = service();
        let session = service
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
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
