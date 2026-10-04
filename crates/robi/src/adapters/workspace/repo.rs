use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use robi_core::ids::WorkspaceId;
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use tracing::error;
use uuid::Uuid;

use crate::domain::{
    error::ServiceError,
    workspace::{model::Workspace, repo::WorkspaceRepository},
};

fn log_unknown(context: &'static str, err: impl std::fmt::Debug + 'static) -> ServiceError {
    if let Some(sql) = (&err as &dyn std::any::Any).downcast_ref::<sqlx::Error>() {
        crate::adapters::sqlite::log_connection_timeout(context, sql);
    }
    error!(?err, %context, "sqlite workspace repository error");
    ServiceError::Unknown
}

fn is_unique(err: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db) = err else {
        return false;
    };
    db.message().contains("UNIQUE")
}

pub struct SqliteWorkspaceRepository {
    pool: Arc<SqlitePool>,
}

impl SqliteWorkspaceRepository {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

fn workspace_from_row(context: &'static str, row: SqliteRow) -> Result<Workspace, ServiceError> {
    let id: String = row.try_get("id").map_err(|err| log_unknown(context, err))?;
    let name: String = row
        .try_get("name")
        .map_err(|err| log_unknown(context, err))?;
    let root: String = row
        .try_get("root")
        .map_err(|err| log_unknown(context, err))?;
    let mcp_project_sha256: Option<String> = row
        .try_get("mcp_project_sha256")
        .map_err(|err| log_unknown(context, err))?;
    let created_at: String = row
        .try_get("created_at")
        .map_err(|err| log_unknown(context, err))?;

    Ok(Workspace {
        id: WorkspaceId::from_uuid(parse_uuid(context, &id)?),
        name,
        root,
        mcp_project_sha256,
        created_at: parse_timestamp(context, &created_at)?,
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

fn canonicalize_blocking(root: &str) -> Result<String, ServiceError> {
    let canonical = std::fs::canonicalize(root)
        .map_err(|_| ServiceError::BadRequest(format!("root does not exist: {root}")))?;
    if !canonical.is_dir() {
        return Err(ServiceError::BadRequest(format!(
            "root must be a directory: {root}"
        )));
    }
    Ok(canonical.to_string_lossy().into_owned())
}

#[async_trait]
impl WorkspaceRepository for SqliteWorkspaceRepository {
    async fn canonicalize_root(&self, root: &str) -> Result<String, ServiceError> {
        let root = root.to_string();
        tokio::task::spawn_blocking(move || canonicalize_blocking(&root))
            .await
            .map_err(|err| log_unknown("canonicalize_root: join", err))?
    }

    async fn get_workspace(&self, id: WorkspaceId) -> Result<Option<Workspace>, ServiceError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, root, mcp_project_sha256, created_at
            FROM workspaces
            WHERE id = ?1
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("get_workspace: select", err))?;

        row.map(|row| workspace_from_row("get_workspace: map row", row))
            .transpose()
    }

    async fn get_workspace_by_root(&self, root: &str) -> Result<Option<Workspace>, ServiceError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, root, mcp_project_sha256, created_at
            FROM workspaces
            WHERE root = ?1
            "#,
        )
        .bind(root)
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("get_workspace_by_root: select", err))?;

        row.map(|row| workspace_from_row("get_workspace_by_root: map row", row))
            .transpose()
    }

    async fn insert_workspace(&self, root: &str, name: &str) -> Result<Workspace, ServiceError> {
        let id = WorkspaceId::new();
        let now = Utc::now();
        let timestamp = now.to_rfc3339();
        let inserted = sqlx::query(
            r#"
            INSERT INTO workspaces (id, name, root, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(id.to_string())
        .bind(name)
        .bind(root)
        .bind(&timestamp)
        .execute(self.pool.as_ref())
        .await;

        match inserted {
            Ok(_) => Ok(Workspace {
                id,
                name: name.to_string(),
                root: root.to_string(),
                mcp_project_sha256: None,
                created_at: now,
            }),
            Err(err) if is_unique(&err) => Err(ServiceError::Conflict(root.to_string())),
            Err(err) => Err(log_unknown("insert_workspace: insert", err)),
        }
    }

    async fn list_workspaces(&self) -> Result<Vec<Workspace>, ServiceError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, root, mcp_project_sha256, created_at
            FROM workspaces
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|err| log_unknown("list_workspaces: select", err))?;

        rows.into_iter()
            .map(|row| workspace_from_row("list_workspaces: map row", row))
            .collect()
    }

    async fn delete_workspace(&self, id: WorkspaceId) -> Result<(), ServiceError> {
        let result = sqlx::query("DELETE FROM workspaces WHERE id = ?1")
            .bind(id.to_string())
            .execute(self.pool.as_ref())
            .await
            .map_err(|err| log_unknown("delete_workspace: delete", err))?;

        if result.rows_affected() == 0 {
            return Err(ServiceError::NotFound(id.to_string()));
        }

        Ok(())
    }

    async fn set_mcp_project_sha256(
        &self,
        id: WorkspaceId,
        hash: Option<String>,
    ) -> Result<(), ServiceError> {
        let result = sqlx::query("UPDATE workspaces SET mcp_project_sha256 = ?1 WHERE id = ?2")
            .bind(hash)
            .bind(id.to_string())
            .execute(self.pool.as_ref())
            .await
            .map_err(|err| log_unknown("set_mcp_project_sha256", err))?;
        if result.rows_affected() == 0 {
            return Err(ServiceError::NotFound(id.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use robi_core::ids::SessionId;
    use uuid::Uuid;

    use super::*;
    use crate::{adapters::sqlite, domain::workspace::service::WorkspaceService};

    async fn service() -> (WorkspaceService, Arc<SqlitePool>) {
        let url = format!(
            "sqlite://file:robi-ws-{}?mode=memory&cache=shared",
            Uuid::now_v7().simple()
        );
        let pool = Arc::new(sqlite::init_pool(&url).await.expect("in-memory pool opens"));
        let service = WorkspaceService {
            repository: Arc::new(SqliteWorkspaceRepository::new(Arc::clone(&pool))),
            asset_cleaner: None,
        };
        (service, pool)
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("robi-ws-{}-{}", name, Uuid::now_v7().simple()));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    #[tokio::test]
    async fn open_workspace_stores_the_canonical_root() {
        let (service, _) = service().await;
        let dir = temp_dir("round");
        let opened = service
            .open_workspace(dir.to_str().expect("utf-8 temp path"))
            .await
            .unwrap();

        let expected = std::fs::canonicalize(&dir).unwrap();
        assert!(opened.created);
        assert_eq!(opened.workspace.root, expected.to_string_lossy());
        assert_eq!(
            opened.workspace.name,
            expected.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(Path::new(&opened.workspace.root), expected.as_path());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn open_workspace_returns_the_same_row_for_one_directory() {
        let (service, pool) = service().await;
        let dir = temp_dir("dup");
        let root = dir.to_str().expect("utf-8 temp path");
        let first = service.open_workspace(root).await.unwrap();
        let second = service.open_workspace(root).await.unwrap();

        assert!(!second.created);
        assert_eq!(first.workspace.id, second.workspace.id);

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspaces")
            .fetch_one(pool.as_ref())
            .await
            .unwrap();
        assert_eq!(count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlink_opens_the_same_workspace() {
        let (service, _) = service().await;
        let dir = temp_dir("link-target");
        let link = std::env::temp_dir().join(format!("robi-ws-link-{}", Uuid::now_v7().simple()));
        std::os::unix::fs::symlink(&dir, &link).expect("symlink");

        let first = service.open_workspace(dir.to_str().unwrap()).await.unwrap();
        let second = service
            .open_workspace(link.to_str().unwrap())
            .await
            .unwrap();
        assert!(!second.created);
        assert_eq!(first.workspace.id, second.workspace.id);

        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn open_workspace_rejects_a_missing_path_and_a_file() {
        let (service, pool) = service().await;
        let missing =
            std::env::temp_dir().join(format!("robi-ws-missing-{}", Uuid::now_v7().simple()));
        let error = service
            .open_workspace(missing.to_str().unwrap())
            .await
            .unwrap_err();
        assert!(matches!(error, ServiceError::BadRequest(_)));

        let file = std::env::temp_dir().join(format!("robi-ws-file-{}", Uuid::now_v7().simple()));
        std::fs::write(&file, b"x").unwrap();
        let error = service
            .open_workspace(file.to_str().unwrap())
            .await
            .unwrap_err();
        assert!(matches!(error, ServiceError::BadRequest(_)));

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspaces")
            .fetch_one(pool.as_ref())
            .await
            .unwrap();
        assert_eq!(count, 0);
        let _ = std::fs::remove_file(&file);
    }

    #[tokio::test]
    async fn list_orders_by_created_at_then_id() {
        let (service, pool) = service().await;
        let early_id = "00000000-0000-7000-8000-000000000001";
        let tie_low = "00000000-0000-7000-8000-000000000002";
        let tie_high = "00000000-0000-7000-8000-000000000003";
        let tied_at = "2024-06-01T00:00:00+00:00";

        for (id, root, created_at) in [
            (early_id, "/work/early", "2020-01-01T00:00:00+00:00"),
            (tie_low, "/work/low", tied_at),
            (tie_high, "/work/high", tied_at),
        ] {
            sqlx::query(
                r#"
                INSERT INTO workspaces (id, name, root, created_at)
                VALUES (?1, ?2, ?3, ?4)
                "#,
            )
            .bind(id)
            .bind(root.rsplit('/').next().unwrap())
            .bind(root)
            .bind(created_at)
            .execute(pool.as_ref())
            .await
            .unwrap();
        }

        let ids: Vec<_> = service
            .list_workspaces()
            .await
            .unwrap()
            .into_iter()
            .map(|workspace| workspace.id.to_string())
            .collect();
        assert_eq!(
            ids,
            vec![
                tie_high.to_string(),
                tie_low.to_string(),
                early_id.to_string()
            ]
        );
    }

    #[tokio::test]
    async fn delete_cascades_sessions_and_messages() {
        let (service, pool) = service().await;
        let dir = temp_dir("cascade");
        let opened = service.open_workspace(dir.to_str().unwrap()).await.unwrap();
        let session_id = SessionId::new().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO chat_sessions (id, workspace_id, title, created_at, updated_at, last_used_at)
            VALUES (?1, ?2, NULL, ?3, ?3, ?3)
            "#,
        )
        .bind(&session_id)
        .bind(opened.workspace.id.to_string())
        .bind(&now)
        .execute(pool.as_ref())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO chat_messages (id, chat_session_id, position, body)
            VALUES (?1, ?2, 0, '{}')
            "#,
        )
        .bind(Uuid::now_v7().to_string())
        .bind(&session_id)
        .execute(pool.as_ref())
        .await
        .unwrap();

        service.delete_workspace(opened.workspace.id).await.unwrap();

        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_sessions")
            .fetch_one(pool.as_ref())
            .await
            .unwrap();
        let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages")
            .fetch_one(pool.as_ref())
            .await
            .unwrap();
        assert_eq!(sessions, 0);
        assert_eq!(messages, 0);

        let missing = WorkspaceId::new();
        assert_eq!(
            service.delete_workspace(missing).await.unwrap_err(),
            ServiceError::NotFound(missing.to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
