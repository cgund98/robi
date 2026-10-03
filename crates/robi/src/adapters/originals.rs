//! `tool_originals` on the session database.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use robi_core::ids::SessionId;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::compress::{sha256_hex, Inserted, Lookup, OriginalStore};

pub struct SqliteOriginals {
    pool: Arc<SqlitePool>,
}

impl SqliteOriginals {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OriginalStore for SqliteOriginals {
    async fn insert(
        &self,
        session: SessionId,
        tool_call_id: &str,
        body: &str,
    ) -> Result<Inserted, String> {
        let id = crate::compress::new_id();
        let sha256 = sha256_hex(body);
        let created_at = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO tool_originals (id, chat_session_id, tool_call_id, sha256, body, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(&id)
        .bind(session.to_string())
        .bind(tool_call_id)
        .bind(&sha256)
        .bind(body)
        .bind(created_at)
        .execute(self.pool.as_ref())
        .await
        .map_err(|err| err.to_string())?;
        Ok(Inserted { id, sha256 })
    }

    async fn lookup(&self, session: SessionId, id: &str) -> Result<Lookup, String> {
        let rows: Vec<(String, String)> = if is_check_digit(id) {
            sqlx::query_as(
                r#"
                SELECT id, body
                FROM tool_originals
                WHERE chat_session_id = ?1 AND substr(sha256, 1, 16) = ?2
                "#,
            )
            .bind(session.to_string())
            .bind(id)
            .fetch_all(self.pool.as_ref())
            .await
            .map_err(|err| err.to_string())?
        } else {
            sqlx::query_as(
                r#"
                SELECT id, body
                FROM tool_originals
                WHERE chat_session_id = ?1 AND id = ?2
                "#,
            )
            .bind(session.to_string())
            .bind(id)
            .fetch_all(self.pool.as_ref())
            .await
            .map_err(|err| err.to_string())?
        };
        rows_to_lookup(rows)
    }
}

fn is_check_digit(id: &str) -> bool {
    id.len() == 16 && id.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn rows_to_lookup(rows: Vec<(String, String)>) -> Result<Lookup, String> {
    match rows.len() {
        0 => Ok(Lookup::Missing),
        1 => {
            let body = serde_json::from_str::<Value>(&rows[0].1).map_err(|err| err.to_string())?;
            Ok(Lookup::One(body))
        }
        _ => Ok(Lookup::Ambiguous(
            rows.into_iter().map(|(id, _)| id).collect(),
        )),
    }
}
