//! `MessageStore` on the `chat_messages` table.
//!
//! `body` is the `serde_json` of `robi_core::Message`. `position` is insertion
//! order. `append` is what moves `chat_sessions.last_used_at`.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use robi_core::error::StoreError;
use robi_core::ids::{MessageId, SessionId, WorkspaceId};
use robi_core::message::Message;
use robi_core::store::MessageStore;
use sqlx::SqlitePool;

pub struct SqliteMessageStore {
    pool: Arc<SqlitePool>,
}

impl SqliteMessageStore {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MessageStore for SqliteMessageStore {
    fn create_session(&self, workspace: WorkspaceId) -> SessionId {
        let id = SessionId::new();
        let pool = Arc::clone(&self.pool);
        if let Err(error) = block_on(insert_session(pool, id, workspace)) {
            tracing::error!(%error, %id, "failed to create chat session");
        }
        id
    }

    fn has_session(&self, session: SessionId) -> bool {
        let pool = Arc::clone(&self.pool);
        match block_on(session_exists(pool, session)) {
            Ok(exists) => exists,
            Err(error) => {
                tracing::error!(%error, %session, "failed to look up chat session");
                false
            }
        }
    }

    async fn messages(&self, session: SessionId) -> Result<Vec<Message>, StoreError> {
        if !session_exists(Arc::clone(&self.pool), session)
            .await
            .map_err(backend)?
        {
            return Err(StoreError::SessionNotFound(session));
        }

        let rows: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT id, body
            FROM chat_messages
            WHERE chat_session_id = ?1
            ORDER BY position ASC
            "#,
        )
        .bind(session.to_string())
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(backend)?;

        rows.into_iter()
            .map(|(id, body)| {
                serde_json::from_str(&body).map_err(|error| {
                    StoreError::Backend(format!("chat message {id} is not a message: {error}"))
                })
            })
            .collect()
    }

    async fn message(
        &self,
        session: SessionId,
        id: MessageId,
    ) -> Result<Option<Message>, StoreError> {
        let body: Option<Option<String>> = sqlx::query_scalar(
            r#"
            SELECT chat_messages.body
            FROM chat_sessions
            LEFT JOIN chat_messages
              ON chat_messages.chat_session_id = chat_sessions.id
             AND chat_messages.id = ?1
            WHERE chat_sessions.id = ?2
            "#,
        )
        .bind(id.to_string())
        .bind(session.to_string())
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(backend)?;

        let Some(body) = body else {
            return Err(StoreError::SessionNotFound(session));
        };
        let Some(body) = body else {
            return Ok(None);
        };

        serde_json::from_str(&body).map(Some).map_err(|error| {
            StoreError::Backend(format!("chat message {id} is not a message: {error}"))
        })
    }

    async fn append(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        if !session_exists(Arc::clone(&self.pool), session)
            .await
            .map_err(backend)?
        {
            return Err(StoreError::SessionNotFound(session));
        }

        let body = serde_json::to_string(&message).map_err(|error| {
            StoreError::Backend(format!(
                "chat message {} did not serialize: {error}",
                message.id
            ))
        })?;
        let now = Utc::now().to_rfc3339();

        let mut tx = self.pool.begin().await.map_err(backend)?;
        let position: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(position), -1) + 1
            FROM chat_messages
            WHERE chat_session_id = ?1
            "#,
        )
        .bind(session.to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;

        sqlx::query(
            r#"
            INSERT INTO chat_messages (id, chat_session_id, position, body)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(message.id.to_string())
        .bind(session.to_string())
        .bind(position)
        .bind(&body)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        sqlx::query(
            r#"
            UPDATE chat_sessions
            SET last_used_at = ?1
            WHERE id = ?2
            "#,
        )
        .bind(&now)
        .bind(session.to_string())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn update(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        if !session_exists(Arc::clone(&self.pool), session)
            .await
            .map_err(backend)?
        {
            return Err(StoreError::SessionNotFound(session));
        }

        let body = serde_json::to_string(&message).map_err(|error| {
            StoreError::Backend(format!(
                "chat message {} did not serialize: {error}",
                message.id
            ))
        })?;
        let result = sqlx::query(
            r#"
            UPDATE chat_messages
            SET body = ?1
            WHERE id = ?2 AND chat_session_id = ?3
            "#,
        )
        .bind(&body)
        .bind(message.id.to_string())
        .bind(session.to_string())
        .execute(self.pool.as_ref())
        .await
        .map_err(backend)?;

        if result.rows_affected() == 0 {
            return Err(StoreError::Backend(format!(
                "chat message {} not found",
                message.id
            )));
        }
        Ok(())
    }
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

async fn insert_session(
    pool: Arc<SqlitePool>,
    id: SessionId,
    workspace: WorkspaceId,
) -> Result<(), sqlx::Error> {
    let timestamp = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO chat_sessions (id, workspace_id, title, created_at, updated_at, last_used_at)
        VALUES (?1, ?2, NULL, ?3, ?3, ?3)
        "#,
    )
    .bind(id.to_string())
    .bind(workspace.to_string())
    .bind(&timestamp)
    .execute(pool.as_ref())
    .await?;
    Ok(())
}

async fn session_exists(pool: Arc<SqlitePool>, session: SessionId) -> Result<bool, sqlx::Error> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM chat_sessions WHERE id = ?1 LIMIT 1")
            .bind(session.to_string())
            .fetch_optional(pool.as_ref())
            .await?;
    Ok(found.is_some())
}

fn backend(error: sqlx::Error) -> StoreError {
    StoreError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use robi_core::ids::{SessionId, WorkspaceId};
    use robi_core::message::Message;
    use robi_core::store::MessageStore;
    use sqlx::SqlitePool;
    use uuid::Uuid;

    use super::SqliteMessageStore;
    use crate::adapters::sqlite;

    async fn store() -> (SqliteMessageStore, Arc<SqlitePool>) {
        let url = format!(
            "sqlite://file:robi-msg-{}?mode=memory&cache=shared",
            Uuid::now_v7().simple()
        );
        let pool = Arc::new(sqlite::init_pool(&url).await.expect("in-memory pool opens"));
        (SqliteMessageStore::new(Arc::clone(&pool)), pool)
    }

    async fn insert_workspace(pool: &SqlitePool, id: WorkspaceId) {
        sqlx::query(
            r#"
            INSERT INTO workspaces (id, name, root, created_at)
            VALUES (?1, 'test', ?2, '2024-01-01T00:00:00+00:00')
            "#,
        )
        .bind(id.to_string())
        .bind(format!("/tmp/robi-msg-{id}"))
        .execute(pool)
        .await
        .expect("insert workspace");
    }

    async fn last_used_at(pool: &SqlitePool, session: SessionId) -> String {
        sqlx::query_scalar("SELECT last_used_at FROM chat_sessions WHERE id = ?1")
            .bind(session.to_string())
            .fetch_one(pool)
            .await
            .expect("last_used_at")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn append_keeps_insertion_order_and_moves_last_used_at() {
        let (store, pool) = store().await;
        let workspace = WorkspaceId::new();
        insert_workspace(&pool, workspace).await;
        let session = store.create_session(workspace);
        assert!(store.has_session(session));

        let before = last_used_at(&pool, session).await;
        tokio::time::sleep(Duration::from_millis(5)).await;

        let first = Message::user("first");
        let second = Message::assistant("second");
        store.append(session, first.clone()).await.unwrap();
        store.append(session, second.clone()).await.unwrap();

        let messages = store.messages(session).await.unwrap();
        assert_eq!(messages, vec![first.clone(), second.clone()]);
        assert_eq!(
            store.message(session, first.id).await.unwrap().as_ref(),
            Some(&first)
        );
        assert_eq!(
            store.message(session, second.id).await.unwrap().as_ref(),
            Some(&second)
        );
        assert_eq!(
            store
                .message(session, robi_core::ids::MessageId::new())
                .await
                .unwrap(),
            None
        );
        assert_ne!(last_used_at(&pool, session).await, before);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn update_replaces_the_body_and_leaves_last_used_at() {
        let (store, pool) = store().await;
        let workspace = WorkspaceId::new();
        insert_workspace(&pool, workspace).await;
        let session = store.create_session(workspace);
        let original = Message::user("original");
        store.append(session, original.clone()).await.unwrap();
        let before = last_used_at(&pool, session).await;

        let mut edited = original.clone();
        edited.content = "edited".to_owned();
        store.update(session, edited.clone()).await.unwrap();

        let messages = store.messages(session).await.unwrap();
        assert_eq!(messages, vec![edited]);
        assert_eq!(last_used_at(&pool, session).await, before);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_missing_session_is_not_found() {
        let (store, _) = store().await;
        let session = SessionId::new();
        assert!(!store.has_session(session));

        let missing = StoreError::SessionNotFound(session);
        assert_eq!(store.messages(session).await.unwrap_err(), missing.clone());
        assert_eq!(
            store
                .message(session, robi_core::ids::MessageId::new())
                .await
                .unwrap_err(),
            missing.clone()
        );
        assert_eq!(
            store
                .append(session, Message::user("hi"))
                .await
                .unwrap_err(),
            missing.clone()
        );
        assert_eq!(
            store
                .update(session, Message::user("hi"))
                .await
                .unwrap_err(),
            missing
        );
    }

    use robi_core::error::StoreError;
}
