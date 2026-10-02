use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use robi_core::ids::{SessionId, WorkspaceId};
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use tracing::error;
use uuid::Uuid;

use crate::domain::{
    chat_session::{
        model::{ChatSession, CreateChatSessionCommand, UpdateChatSessionCommand},
        repo::ChatSessionRepository,
    },
    error::ServiceError,
};

fn log_unknown(context: &'static str, err: impl std::fmt::Debug) -> ServiceError {
    error!(?err, %context, "sqlite chat session repository error");
    ServiceError::Unknown
}

fn chat_session_from_row(
    context: &'static str,
    row: SqliteRow,
) -> Result<ChatSession, ServiceError> {
    let id: String = row.try_get("id").map_err(|err| log_unknown(context, err))?;
    let workspace_id: String = row
        .try_get("workspace_id")
        .map_err(|err| log_unknown(context, err))?;
    let title: Option<String> = row
        .try_get("title")
        .map_err(|err| log_unknown(context, err))?;
    let created_at: String = row
        .try_get("created_at")
        .map_err(|err| log_unknown(context, err))?;
    let updated_at: String = row
        .try_get("updated_at")
        .map_err(|err| log_unknown(context, err))?;
    let last_used_at: String = row
        .try_get("last_used_at")
        .map_err(|err| log_unknown(context, err))?;

    Ok(ChatSession {
        id: SessionId::from_uuid(parse_uuid(context, &id)?),
        workspace_id: WorkspaceId::from_uuid(parse_uuid(context, &workspace_id)?),
        title,
        created_at: parse_timestamp(context, &created_at)?,
        updated_at: parse_timestamp(context, &updated_at)?,
        last_used_at: parse_timestamp(context, &last_used_at)?,
    })
}

fn parse_uuid(context: &'static str, value: &str) -> Result<Uuid, ServiceError> {
    Uuid::parse_str(value).map_err(|err| log_unknown(context, err))
}

fn parse_timestamp(context: &'static str, value: &str) -> Result<DateTime<Utc>, ServiceError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|err| log_unknown(context, err))
}

pub struct SqliteChatSessionRepository {
    pool: Arc<SqlitePool>,
}

impl SqliteChatSessionRepository {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ChatSessionRepository for SqliteChatSessionRepository {
    async fn create_chat_session(
        &self,
        command: CreateChatSessionCommand,
    ) -> Result<ChatSession, ServiceError> {
        let id = SessionId::new();
        let now = Utc::now();
        let timestamp = now.to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO chat_sessions (id, workspace_id, title, created_at, updated_at, last_used_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(id.to_string())
        .bind(command.workspace_id.to_string())
        .bind(&command.title)
        .bind(&timestamp)
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("create_chat_session: insert", err))?;

        Ok(ChatSession {
            id,
            workspace_id: command.workspace_id,
            title: command.title,
            created_at: now,
            updated_at: now,
            last_used_at: now,
        })
    }

    async fn get_chat_session(&self, id: SessionId) -> Result<Option<ChatSession>, ServiceError> {
        let row = sqlx::query(
            r#"
            SELECT id, workspace_id, title, created_at, updated_at, last_used_at
            FROM chat_sessions
            WHERE id = ?1
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("get_chat_session: select", err))?;

        row.map(|row| chat_session_from_row("get_chat_session: map row", row))
            .transpose()
    }

    async fn list_chat_sessions(
        &self,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<Vec<ChatSession>, ServiceError> {
        let rows = sqlx::query(
            r#"
            SELECT id, workspace_id, title, created_at, updated_at, last_used_at
            FROM chat_sessions
            WHERE (?1 IS NULL OR workspace_id = ?1)
            ORDER BY last_used_at DESC, id DESC
            "#,
        )
        .bind(workspace_id.map(|id| id.to_string()))
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("list_chat_sessions: select", err))?;

        rows.into_iter()
            .map(|row| chat_session_from_row("list_chat_sessions: map row", row))
            .collect()
    }

    async fn update_chat_session(
        &self,
        command: UpdateChatSessionCommand,
    ) -> Result<ChatSession, ServiceError> {
        let updated_at = Utc::now().to_rfc3339();
        let result = sqlx::query(
            r#"
            UPDATE chat_sessions
            SET title = ?1, updated_at = ?2
            WHERE id = ?3
            "#,
        )
        .bind(&command.title)
        .bind(&updated_at)
        .bind(command.id.to_string())
        .execute(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("update_chat_session: update", err))?;

        if result.rows_affected() == 0 {
            return Err(ServiceError::NotFound(command.id.to_string()));
        }

        self.get_chat_session(command.id)
            .await?
            .ok_or_else(|| ServiceError::NotFound(command.id.to_string()))
    }

    async fn delete_chat_session(&self, id: SessionId) -> Result<(), ServiceError> {
        let result = sqlx::query("DELETE FROM chat_sessions WHERE id = ?1")
            .bind(id.to_string())
            .execute(self.pool.as_ref())
            .await
            .map_err(|err| log_unknown("delete_chat_session: delete", err))?;

        if result.rows_affected() == 0 {
            return Err(ServiceError::NotFound(id.to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use robi_core::ids::{SessionId, WorkspaceId};
    use sqlx::SqlitePool;
    use uuid::Uuid;

    use super::*;
    use crate::adapters::sqlite;

    async fn repository() -> SqliteChatSessionRepository {
        let url = format!(
            "sqlite://file:robi-{}?mode=memory&cache=shared",
            Uuid::now_v7().simple()
        );
        let pool = sqlite::init_pool(&url).await.expect("in-memory pool opens");
        SqliteChatSessionRepository::new(Arc::new(pool))
    }

    async fn insert_chat_session(
        pool: &SqlitePool,
        id: &str,
        workspace_id: &str,
        last_used_at: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO chat_sessions (id, workspace_id, title, created_at, updated_at, last_used_at)
            VALUES (?1, ?2, '', ?3, ?3, ?4)
            "#,
        )
        .bind(id)
        .bind(workspace_id)
        .bind(last_used_at)
        .bind(last_used_at)
        .execute(pool)
        .await
        .expect("insert session");
    }

    #[tokio::test]
    async fn create_and_get_round_trip() {
        let repo = repository().await;
        let workspace_id = WorkspaceId::new();
        let created = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: Some("Round trip".into()),
            })
            .await
            .unwrap();

        let loaded = repo.get_chat_session(created.id).await.unwrap().unwrap();
        assert_eq!(loaded, created);
        assert!(repo
            .get_chat_session(SessionId::new())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn create_without_a_title_stores_null() {
        let repo = repository().await;
        let created = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
            })
            .await
            .unwrap();

        assert_eq!(created.title, None);
        let loaded = repo.get_chat_session(created.id).await.unwrap().unwrap();
        assert_eq!(loaded.title, None);
    }

    #[tokio::test]
    async fn list_orders_by_last_used_then_id() {
        let repo = repository().await;
        let workspace_id = WorkspaceId::new().to_string();
        let early = "00000000-0000-7000-8000-000000000001";
        let tie_low = "00000000-0000-7000-8000-000000000002";
        let tie_mid = "00000000-0000-7000-8000-000000000003";
        let tie_high = "00000000-0000-7000-8000-000000000004";
        let tied_at = "2024-01-01T00:00:00+00:00";

        insert_chat_session(
            repo.pool.as_ref(),
            early,
            &workspace_id,
            "2020-01-01T00:00:00+00:00",
        )
        .await;
        insert_chat_session(repo.pool.as_ref(), tie_low, &workspace_id, tied_at).await;
        insert_chat_session(repo.pool.as_ref(), tie_mid, &workspace_id, tied_at).await;
        insert_chat_session(repo.pool.as_ref(), tie_high, &workspace_id, tied_at).await;

        let ids: Vec<_> = repo
            .list_chat_sessions(None)
            .await
            .unwrap()
            .into_iter()
            .map(|session| session.id.to_string())
            .collect();

        assert_eq!(
            ids,
            vec![
                tie_high.to_string(),
                tie_mid.to_string(),
                tie_low.to_string(),
                early.to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn list_filters_by_workspace() {
        let repo = repository().await;
        let keep = WorkspaceId::new();
        let other = WorkspaceId::new();
        insert_chat_session(
            repo.pool.as_ref(),
            &SessionId::new().to_string(),
            &keep.to_string(),
            "2024-01-01T00:00:00+00:00",
        )
        .await;
        insert_chat_session(
            repo.pool.as_ref(),
            &SessionId::new().to_string(),
            &other.to_string(),
            "2024-06-01T00:00:00+00:00",
        )
        .await;

        let listed = repo.list_chat_sessions(Some(keep)).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].workspace_id, keep);
    }

    #[tokio::test]
    async fn update_changes_title_and_leaves_last_used_at() {
        let repo = repository().await;
        let created = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: Some("Before".into()),
            })
            .await
            .unwrap();

        let updated = repo
            .update_chat_session(UpdateChatSessionCommand {
                id: created.id,
                title: "After".into(),
            })
            .await
            .unwrap();

        assert_eq!(updated.title.as_deref(), Some("After"));
        assert_eq!(updated.last_used_at, created.last_used_at);
        assert_eq!(updated.created_at, created.created_at);
        assert_eq!(updated.workspace_id, created.workspace_id);
        assert!(updated.updated_at >= created.updated_at);

        let missing = SessionId::new();
        assert_eq!(
            repo.update_chat_session(UpdateChatSessionCommand {
                id: missing,
                title: "Nope".into(),
            })
            .await
            .unwrap_err(),
            ServiceError::NotFound(missing.to_string())
        );
    }

    #[tokio::test]
    async fn delete_missing_is_not_found_and_present_cascades_messages() {
        let repo = repository().await;
        let missing = SessionId::new();
        assert_eq!(
            repo.delete_chat_session(missing).await.unwrap_err(),
            ServiceError::NotFound(missing.to_string())
        );

        let session = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: WorkspaceId::new(),
                title: None,
            })
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO chat_messages (id, chat_session_id, position, body)
            VALUES (?1, ?2, 0, '{}')
            "#,
        )
        .bind(Uuid::now_v7().to_string())
        .bind(session.id.to_string())
        .execute(repo.pool.as_ref())
        .await
        .expect("insert message");

        repo.delete_chat_session(session.id).await.unwrap();

        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE chat_session_id = ?1")
                .bind(session.id.to_string())
                .fetch_one(repo.pool.as_ref())
                .await
                .expect("count messages");
        assert_eq!(remaining, 0);
        assert!(repo.get_chat_session(session.id).await.unwrap().is_none());
    }
}
