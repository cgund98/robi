use std::sync::Arc;

use async_trait::async_trait;
use robi_core::ids::SessionId;
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use tracing::error;

use crate::domain::{
    error::ServiceError,
    file_change::{model::FileBaseline, repo::FileChangeRepository},
};

pub struct SqliteFileChangeRepository {
    pool: Arc<SqlitePool>,
}

impl SqliteFileChangeRepository {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

fn log_unknown(context: &'static str, err: impl std::fmt::Debug) -> ServiceError {
    error!(?err, %context, "sqlite file baseline repository error");
    ServiceError::Unknown
}

fn is_foreign_key(err: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db) = err else {
        return false;
    };
    db.code().as_deref() == Some("787") || db.message().contains("FOREIGN KEY")
}

fn baseline_from_row(context: &'static str, row: SqliteRow) -> Result<FileBaseline, ServiceError> {
    let path: String = row
        .try_get("path")
        .map_err(|err| log_unknown(context, err))?;
    let baseline: String = row
        .try_get("baseline")
        .map_err(|err| log_unknown(context, err))?;
    let created: i64 = row
        .try_get("created")
        .map_err(|err| log_unknown(context, err))?;
    Ok(FileBaseline {
        path,
        baseline,
        created: created != 0,
    })
}

#[async_trait]
impl FileChangeRepository for SqliteFileChangeRepository {
    async fn record_baseline(
        &self,
        session_id: SessionId,
        path: &str,
        baseline: &str,
        created: bool,
    ) -> Result<bool, ServiceError> {
        let inserted = sqlx::query(
            r#"
            INSERT OR IGNORE INTO session_file_baselines
                (chat_session_id, path, baseline, created)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(session_id.to_string())
        .bind(path)
        .bind(baseline)
        .bind(i64::from(created))
        .execute(self.pool.as_ref())
        .await;

        match inserted {
            Ok(result) => Ok(result.rows_affected() == 1),
            Err(err) if is_foreign_key(&err) => Err(ServiceError::NotFound(session_id.to_string())),
            Err(err) => Err(log_unknown("record_baseline: insert", err)),
        }
    }

    async fn get_baseline(
        &self,
        session_id: SessionId,
        path: &str,
    ) -> Result<Option<FileBaseline>, ServiceError> {
        let row = sqlx::query(
            r#"
            SELECT path, baseline, created
            FROM session_file_baselines
            WHERE chat_session_id = ?1 AND path = ?2
            "#,
        )
        .bind(session_id.to_string())
        .bind(path)
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("get_baseline: select", err))?;

        row.map(|row| baseline_from_row("get_baseline: map row", row))
            .transpose()
    }

    async fn list_baselines(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<FileBaseline>, ServiceError> {
        let rows = sqlx::query(
            r#"
            SELECT path, baseline, created
            FROM session_file_baselines
            WHERE chat_session_id = ?1
            ORDER BY path ASC
            "#,
        )
        .bind(session_id.to_string())
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("list_baselines: select", err))?;

        rows.into_iter()
            .map(|row| baseline_from_row("list_baselines: map row", row))
            .collect()
    }

    async fn delete_baseline(&self, session_id: SessionId, path: &str) -> Result<(), ServiceError> {
        sqlx::query(
            r#"
            DELETE FROM session_file_baselines
            WHERE chat_session_id = ?1 AND path = ?2
            "#,
        )
        .bind(session_id.to_string())
        .bind(path)
        .execute(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("delete_baseline: delete", err))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use robi_core::ids::SessionId;
    use uuid::Uuid;

    use super::*;
    use crate::adapters::{
        chat_session::repo::SqliteChatSessionRepository, sqlite,
        workspace::repo::SqliteWorkspaceRepository,
    };
    use crate::domain::{
        chat_session::{
            model::CreateChatSessionCommand, repo::ChatSessionRepository,
            service::ChatSessionService,
        },
        workspace::service::WorkspaceService,
    };

    async fn session() -> (Arc<SqlitePool>, SessionId) {
        let url = format!(
            "sqlite://file:robi-base-{}?mode=memory&cache=shared",
            Uuid::now_v7().simple()
        );
        let pool = Arc::new(sqlite::init_pool(&url).await.expect("pool"));
        let dir = std::env::temp_dir().join(format!("robi-base-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let workspaces = Arc::new(SqliteWorkspaceRepository::new(Arc::clone(&pool)));
        let workspace = WorkspaceService {
            repository: workspaces.clone(),
        }
        .open_workspace(dir.to_str().unwrap())
        .await
        .unwrap()
        .workspace;
        let sessions = ChatSessionService {
            repository: Arc::new(SqliteChatSessionRepository::new(Arc::clone(&pool))),
            workspaces,
        };
        let chat = sessions
            .create_chat_session(CreateChatSessionCommand {
                workspace_id: workspace.id,
                title: None,
                model_config: crate::domain::chat_session::model::ModelConfig::default(),
            })
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(dir);
        (pool, chat.id)
    }

    #[tokio::test]
    async fn the_first_baseline_is_kept() {
        let (pool, session_id) = session().await;
        let repo = SqliteFileChangeRepository::new(pool);
        assert!(repo
            .record_baseline(session_id, "src/a.rs", "one\n", false)
            .await
            .unwrap());
        assert!(!repo
            .record_baseline(session_id, "src/a.rs", "two\n", false)
            .await
            .unwrap());
        let stored = repo.get_baseline(session_id, "src/a.rs").await.unwrap();
        assert_eq!(stored.unwrap().baseline, "one\n");
    }

    #[tokio::test]
    async fn deleting_the_session_drops_its_baselines() {
        let (pool, session_id) = session().await;
        let repo = SqliteFileChangeRepository::new(Arc::clone(&pool));
        repo.record_baseline(session_id, "a.txt", "hi\n", false)
            .await
            .unwrap();
        SqliteChatSessionRepository::new(pool)
            .delete_chat_session(session_id)
            .await
            .unwrap();
        let listed = repo.list_baselines(session_id).await.unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn a_missing_session_is_not_found() {
        let (pool, _) = session().await;
        let repo = SqliteFileChangeRepository::new(pool);
        let error = repo
            .record_baseline(SessionId::from_uuid(Uuid::now_v7()), "a.txt", "", true)
            .await
            .unwrap_err();
        assert!(matches!(error, ServiceError::NotFound(_)));
    }
}
