use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use robi_core::ids::{SessionId, WorkspaceId};
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use tracing::error;
use uuid::Uuid;

use crate::domain::{
    chat_session::{
        model::{
            apply_session_update, AgentMode, ChatSession, CreateChatSessionCommand, ModelConfig,
            PathRules, UpdateChatSessionCommand,
        },
        repo::ChatSessionRepository,
    },
    error::ServiceError,
};

const SESSION_COLUMNS: &str = "id, workspace_id, title, path_allow_read, path_allow_write, path_deny_read, path_deny_write, allow_hosts, mode, model_config, plan_path, created_at, updated_at, last_used_at";

fn log_unknown(context: &'static str, err: impl std::fmt::Debug) -> ServiceError {
    error!(?err, %context, "sqlite chat session repository error");
    ServiceError::Unknown
}

fn is_foreign_key(err: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db) = err else {
        return false;
    };
    db.code().as_deref() == Some("787") || db.message().contains("FOREIGN KEY")
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
    let path_rules = PathRules {
        allow_read: decode_patterns(context, &row_text(context, &row, "path_allow_read")?)?,
        allow_write: decode_patterns(context, &row_text(context, &row, "path_allow_write")?)?,
        deny_read: decode_patterns(context, &row_text(context, &row, "path_deny_read")?)?,
        deny_write: decode_patterns(context, &row_text(context, &row, "path_deny_write")?)?,
    };
    let allow_hosts = decode_patterns(context, &row_text(context, &row, "allow_hosts")?)?;
    let mode = AgentMode::parse(&row_text(context, &row, "mode")?)
        .map_err(|err| log_unknown(context, err))?;
    let model_config = decode_model_config(context, &row_text(context, &row, "model_config")?)?;
    let plan_path: Option<String> = row
        .try_get("plan_path")
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
        path_rules,
        allow_hosts,
        mode,
        model_config,
        plan_path,
        created_at: parse_timestamp(context, &created_at)?,
        updated_at: parse_timestamp(context, &updated_at)?,
        last_used_at: parse_timestamp(context, &last_used_at)?,
    })
}

fn row_text(
    context: &'static str,
    row: &SqliteRow,
    column: &'static str,
) -> Result<String, ServiceError> {
    row.try_get(column).map_err(|err| log_unknown(context, err))
}

fn decode_patterns(context: &'static str, value: &str) -> Result<Vec<String>, ServiceError> {
    serde_json::from_str(value).map_err(|err| log_unknown(context, err))
}

fn encode_patterns(patterns: &[String]) -> Result<String, ServiceError> {
    serde_json::to_string(patterns).map_err(|err| log_unknown("encode path rules", err))
}

fn decode_model_config(context: &'static str, value: &str) -> Result<ModelConfig, ServiceError> {
    serde_json::from_str(value).map_err(|err| log_unknown(context, err))
}

fn encode_model_config(config: &ModelConfig) -> Result<String, ServiceError> {
    serde_json::to_string(config).map_err(|err| log_unknown("encode model config", err))
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
        let path_rules = PathRules::default();
        let allow_read = encode_patterns(&path_rules.allow_read)?;
        let allow_write = encode_patterns(&path_rules.allow_write)?;
        let deny_read = encode_patterns(&path_rules.deny_read)?;
        let deny_write = encode_patterns(&path_rules.deny_write)?;
        let allow_hosts = encode_patterns(&[])?;
        let model_config = encode_model_config(&command.model_config)?;

        sqlx::query(
            r#"
            INSERT INTO chat_sessions (
                id, workspace_id, title, path_allow_read, path_allow_write,
                path_deny_read, path_deny_write, allow_hosts, mode, model_config, created_at,
                updated_at, last_used_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        )
        .bind(id.to_string())
        .bind(command.workspace_id.to_string())
        .bind(&command.title)
        .bind(&allow_read)
        .bind(&allow_write)
        .bind(&deny_read)
        .bind(&deny_write)
        .bind(&allow_hosts)
        .bind(command.mode.as_str())
        .bind(&model_config)
        .bind(&timestamp)
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(self.pool.as_ref())
        .await
        .map_err(|err| {
            if is_foreign_key(&err) {
                return ServiceError::NotFound(command.workspace_id.to_string());
            }
            log_unknown("create_chat_session: insert", err)
        })?;

        Ok(ChatSession {
            id,
            workspace_id: command.workspace_id,
            title: command.title,
            path_rules,
            allow_hosts: Vec::new(),
            mode: command.mode,
            model_config: command.model_config,
            plan_path: None,
            created_at: now,
            updated_at: now,
            last_used_at: now,
        })
    }

    async fn get_chat_session(&self, id: SessionId) -> Result<Option<ChatSession>, ServiceError> {
        let row = sqlx::query(&format!(
            "SELECT {SESSION_COLUMNS} FROM chat_sessions WHERE id = ?1"
        ))
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
        let rows = sqlx::query(&format!(
            "SELECT {SESSION_COLUMNS} FROM chat_sessions WHERE (?1 IS NULL OR workspace_id = ?1) ORDER BY last_used_at DESC, id DESC"
        ))
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
        let Some(mut session) = self.get_chat_session(command.id).await? else {
            return Err(ServiceError::NotFound(command.id.to_string()));
        };
        if !apply_session_update(&mut session, &command) {
            return Ok(session);
        }
        session.updated_at = Utc::now();
        let allow_read = encode_patterns(&session.path_rules.allow_read)?;
        let allow_write = encode_patterns(&session.path_rules.allow_write)?;
        let deny_read = encode_patterns(&session.path_rules.deny_read)?;
        let deny_write = encode_patterns(&session.path_rules.deny_write)?;
        let allow_hosts = encode_patterns(&session.allow_hosts)?;
        let model_config = encode_model_config(&session.model_config)?;
        sqlx::query(
            r#"
            UPDATE chat_sessions
            SET title = ?1,
                path_allow_read = ?2,
                path_allow_write = ?3,
                path_deny_read = ?4,
                path_deny_write = ?5,
                allow_hosts = ?6,
                mode = ?7,
                model_config = ?8,
                updated_at = ?9
            WHERE id = ?10
            "#,
        )
        .bind(&session.title)
        .bind(&allow_read)
        .bind(&allow_write)
        .bind(&deny_read)
        .bind(&deny_write)
        .bind(&allow_hosts)
        .bind(session.mode.as_str())
        .bind(&model_config)
        .bind(session.updated_at.to_rfc3339())
        .bind(command.id.to_string())
        .execute(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("update_chat_session: update", err))?;

        Ok(session)
    }

    async fn set_title_if_unset(
        &self,
        id: SessionId,
        title: String,
    ) -> Result<Option<ChatSession>, ServiceError> {
        let updated_at = Utc::now().to_rfc3339();
        let result = sqlx::query(
            r#"
            UPDATE chat_sessions
            SET title = ?1, updated_at = ?2
            WHERE id = ?3 AND title IS NULL
            "#,
        )
        .bind(&title)
        .bind(&updated_at)
        .bind(id.to_string())
        .execute(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("set_title_if_unset: update", err))?;

        if result.rows_affected() == 1 {
            let session = self
                .get_chat_session(id)
                .await?
                .ok_or_else(|| ServiceError::NotFound(id.to_string()))?;
            return Ok(Some(session));
        }

        match self.get_chat_session(id).await? {
            Some(_) => Ok(None),
            None => Err(ServiceError::NotFound(id.to_string())),
        }
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

    async fn set_plan_path(&self, id: SessionId, path: String) -> Result<(), ServiceError> {
        let result = sqlx::query(
            r#"
            UPDATE chat_sessions
            SET plan_path = ?1
            WHERE id = ?2
            "#,
        )
        .bind(&path)
        .bind(id.to_string())
        .execute(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("set_plan_path: update", err))?;
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

    async fn insert_workspace(pool: &SqlitePool, id: &str) {
        sqlx::query(
            r#"
            INSERT INTO workspaces (id, name, root, created_at)
            VALUES (?1, 'test', ?2, '2024-01-01T00:00:00+00:00')
            "#,
        )
        .bind(id)
        .bind(format!("/tmp/robi-test-{id}"))
        .execute(pool)
        .await
        .expect("insert workspace");
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
        insert_workspace(repo.pool.as_ref(), &workspace_id.to_string()).await;
        let created = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: Some("Round trip".into()),
                mode: AgentMode::Plan,
                model_config: ModelConfig {
                    plan: crate::domain::chat_session::model::ModeOverride {
                        model: Some("glm-5.2".into()),
                        reasoning_effort: Some("high".into()),
                    },
                    ..ModelConfig::default()
                },
            })
            .await
            .unwrap();

        let loaded = repo.get_chat_session(created.id).await.unwrap().unwrap();
        assert_eq!(loaded, created);
        assert_eq!(loaded.plan_path, None);
        let updated_at = loaded.updated_at;
        repo.set_plan_path(created.id, ".robi/plans/ship.md".into())
            .await
            .unwrap();
        let pointed = repo.get_chat_session(created.id).await.unwrap().unwrap();
        assert_eq!(pointed.plan_path.as_deref(), Some(".robi/plans/ship.md"));
        assert_eq!(pointed.updated_at, updated_at);
        assert_eq!(pointed.last_used_at, loaded.last_used_at);
        assert!(repo
            .get_chat_session(SessionId::new())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn create_without_a_title_stores_null() {
        let repo = repository().await;
        let workspace_id = WorkspaceId::new();
        insert_workspace(repo.pool.as_ref(), &workspace_id.to_string()).await;
        let created = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
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
        insert_workspace(repo.pool.as_ref(), &workspace_id).await;
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
        insert_workspace(repo.pool.as_ref(), &keep.to_string()).await;
        insert_workspace(repo.pool.as_ref(), &other.to_string()).await;
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
        let workspace_id = WorkspaceId::new();
        insert_workspace(repo.pool.as_ref(), &workspace_id.to_string()).await;
        let created = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: Some("Before".into()),
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();

        let updated = repo
            .update_chat_session(UpdateChatSessionCommand::rename(created.id, "After"))
            .await
            .unwrap();

        assert_eq!(updated.title.as_deref(), Some("After"));
        assert_eq!(updated.last_used_at, created.last_used_at);
        assert_eq!(updated.created_at, created.created_at);
        assert_eq!(updated.workspace_id, created.workspace_id);
        assert!(updated.updated_at >= created.updated_at);

        let missing = SessionId::new();
        assert_eq!(
            repo.update_chat_session(UpdateChatSessionCommand::rename(missing, "Nope"))
                .await
                .unwrap_err(),
            ServiceError::NotFound(missing.to_string())
        );
    }

    #[tokio::test]
    async fn set_title_if_unset_leaves_an_existing_title_and_last_used_at() {
        let repo = repository().await;
        let workspace_id = WorkspaceId::new();
        insert_workspace(repo.pool.as_ref(), &workspace_id.to_string()).await;
        let created = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();

        let named = repo
            .set_title_if_unset(created.id, "Parser cleanup".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(named.title.as_deref(), Some("Parser cleanup"));
        assert_eq!(named.last_used_at, created.last_used_at);
        assert!(named.updated_at >= created.updated_at);

        assert!(repo
            .set_title_if_unset(created.id, "Other".into())
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            repo.get_chat_session(created.id)
                .await
                .unwrap()
                .unwrap()
                .title
                .as_deref(),
            Some("Parser cleanup")
        );

        let missing = SessionId::new();
        assert_eq!(
            repo.set_title_if_unset(missing, "Nope".into())
                .await
                .unwrap_err(),
            ServiceError::NotFound(missing.to_string())
        );
    }

    #[test]
    fn path_rule_columns_default_to_empty_lists() {
        let sql = include_str!("../../../migrations/0001_chat_sessions.sql");
        assert!(sql.contains("path_deny_read TEXT NOT NULL DEFAULT '[]'"));
        assert!(sql.contains("path_deny_write TEXT NOT NULL DEFAULT '[]'"));
    }

    #[tokio::test]
    async fn create_stores_default_path_rules_and_patch_replaces_one_list() {
        let repo = repository().await;
        let workspace_id = WorkspaceId::new();
        insert_workspace(repo.pool.as_ref(), &workspace_id.to_string()).await;
        let created = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap();
        assert_eq!(created.path_rules, PathRules::default());

        let updated = repo
            .update_chat_session(UpdateChatSessionCommand {
                id: created.id,
                title: None,
                allow_read: Some(vec![r"^src/\.env$".into()]),
                allow_write: None,
                deny_read: None,
                deny_write: None,
                allow_hosts: None,
                mode: None,
                model_config: None,
            })
            .await
            .unwrap();
        assert_eq!(updated.path_rules.allow_read, vec![r"^src/\.env$"]);
        assert!(updated.path_rules.allow_write.is_empty());
        assert_eq!(updated.path_rules.deny_read, created.path_rules.deny_read);
        assert_eq!(updated.last_used_at, created.last_used_at);
        assert!(updated.updated_at >= created.updated_at);
    }

    #[tokio::test]
    async fn delete_missing_is_not_found_and_present_cascades_messages() {
        let repo = repository().await;
        let missing = SessionId::new();
        assert_eq!(
            repo.delete_chat_session(missing).await.unwrap_err(),
            ServiceError::NotFound(missing.to_string())
        );

        let workspace_id = WorkspaceId::new();
        insert_workspace(repo.pool.as_ref(), &workspace_id.to_string()).await;
        let session = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
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

    #[tokio::test]
    async fn create_fails_when_the_workspace_row_is_absent() {
        let repo = repository().await;
        let workspace_id = WorkspaceId::new();
        let error = repo
            .create_chat_session(CreateChatSessionCommand {
                workspace_id,
                title: None,
                mode: AgentMode::Agent,
                model_config: ModelConfig::default(),
            })
            .await
            .unwrap_err();
        assert_eq!(error, ServiceError::NotFound(workspace_id.to_string()));
    }
}
