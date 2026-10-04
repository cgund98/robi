//! Large values for one chat session, outside the shared SQLite file.
//!
//! Each session gets `sessions/<id>/blobs.redb` under the app home, the same
//! isolation a workspace index has. Tool originals, tool results, and image
//! bytes live there. A write in one session does not take the chat database
//! lock. A small `image-index.redb` in the sessions directory maps an image id
//! to its session so a lookup that only has the id can open the right file.
//! Deleting a session removes its directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use redb::{Database, ReadableTable, TableDefinition};
use robi_core::ids::{MessageId, SessionId};
use robi_core::message::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const ORIGINALS: TableDefinition<&str, &str> = TableDefinition::new("originals");
const ORIGINALS_BY_SHA: TableDefinition<&str, &str> = TableDefinition::new("originals_by_sha");
const RESULTS: TableDefinition<&str, &str> = TableDefinition::new("results");
const IMAGES: TableDefinition<&str, &[u8]> = TableDefinition::new("images");
const IMAGE_TYPES: TableDefinition<&str, &str> = TableDefinition::new("image_types");
const IMAGE_INDEX: TableDefinition<&str, &str> = TableDefinition::new("image_index");

#[derive(Clone)]
pub struct SessionBlobs {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    dbs: Mutex<HashMap<String, Arc<Database>>>,
    image_index: Mutex<Option<Arc<Database>>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OriginalRecord {
    tool_call_id: String,
    sha256: String,
    body: String,
    created_at: String,
}

impl SessionBlobs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                root: root.into(),
                dbs: Mutex::new(HashMap::new()),
                image_index: Mutex::new(None),
            }),
        }
    }

    /// `~/.robi/sessions`.
    pub fn directory(settings_dir: &Path) -> PathBuf {
        settings_dir.join("sessions")
    }

    pub fn insert_original(
        &self,
        session: SessionId,
        id: &str,
        tool_call_id: &str,
        sha256: &str,
        body: &str,
        created_at: &str,
    ) -> Result<(), String> {
        let record = serde_json::to_string(&OriginalRecord {
            tool_call_id: tool_call_id.to_owned(),
            sha256: sha256.to_owned(),
            body: body.to_owned(),
            created_at: created_at.to_owned(),
        })
        .map_err(|err| err.to_string())?;
        let sha_key = sha_key(sha256, id);
        let db = self.session_db(session)?;
        let write = db.begin_write().map_err(redb_err)?;
        {
            let mut originals = write.open_table(ORIGINALS).map_err(redb_err)?;
            originals.insert(id, record.as_str()).map_err(redb_err)?;
            let mut by_sha = write.open_table(ORIGINALS_BY_SHA).map_err(redb_err)?;
            by_sha.insert(sha_key.as_str(), id).map_err(redb_err)?;
        }
        write.commit().map_err(redb_err)?;
        Ok(())
    }

    /// Bodies whose id matches, or whose sha starts with a 16-hex prefix.
    pub fn lookup_originals(
        &self,
        session: SessionId,
        id_or_prefix: &str,
    ) -> Result<Vec<(String, String)>, String> {
        let db = match self.session_db_if_present(session)? {
            Some(db) => db,
            None => return Ok(Vec::new()),
        };
        let read = db.begin_read().map_err(redb_err)?;
        let originals = read.open_table(ORIGINALS).map_err(redb_err)?;
        if is_sha_prefix(id_or_prefix) {
            let by_sha = read.open_table(ORIGINALS_BY_SHA).map_err(redb_err)?;
            let upper = prefix_upper(id_or_prefix);
            let range = by_sha
                .range(id_or_prefix..upper.as_str())
                .map_err(redb_err)?;
            let mut rows = Vec::new();
            for item in range {
                let (_key, id) = item.map_err(redb_err)?;
                let id = id.value().to_owned();
                if let Some(record) = originals.get(id.as_str()).map_err(redb_err)? {
                    let record: OriginalRecord =
                        serde_json::from_str(record.value()).map_err(|err| err.to_string())?;
                    rows.push((id, record.body));
                }
            }
            return Ok(rows);
        }
        match originals.get(id_or_prefix).map_err(redb_err)? {
            Some(record) => {
                let record: OriginalRecord =
                    serde_json::from_str(record.value()).map_err(|err| err.to_string())?;
                Ok(vec![(id_or_prefix.to_owned(), record.body)])
            }
            None => Ok(Vec::new()),
        }
    }

    /// Replace the stored results for this message. A call with no result drops
    /// its key so a later read does not resurrect it.
    pub fn sync_results(&self, session: SessionId, message: &Message) -> Result<(), String> {
        let any = message.tool_calls.iter().any(|call| call.result.is_some());
        if !any && self.session_db_if_present(session)?.is_none() {
            return Ok(());
        }
        let db = self.session_db(session)?;
        let write = db.begin_write().map_err(redb_err)?;
        {
            let mut table = write.open_table(RESULTS).map_err(redb_err)?;
            for call in &message.tool_calls {
                let key = result_key(&message.id, &call.id.to_string());
                match &call.result {
                    Some(value) => {
                        let body = serde_json::to_string(value).map_err(|err| err.to_string())?;
                        table
                            .insert(key.as_str(), body.as_str())
                            .map_err(redb_err)?;
                    }
                    None => {
                        table.remove(key.as_str()).map_err(redb_err)?;
                    }
                }
            }
        }
        write.commit().map_err(redb_err)?;
        Ok(())
    }

    pub fn fill_results(&self, session: SessionId, message: &mut Message) -> Result<(), String> {
        if message.tool_calls.is_empty() {
            return Ok(());
        }
        let Some(db) = self.session_db_if_present(session)? else {
            return Ok(());
        };
        let read = db.begin_read().map_err(redb_err)?;
        let table = read.open_table(RESULTS).map_err(redb_err)?;
        for call in &mut message.tool_calls {
            if call.result.is_some() {
                continue;
            }
            let key = result_key(&message.id, &call.id.to_string());
            if let Some(body) = table.get(key.as_str()).map_err(redb_err)? {
                let value: Value =
                    serde_json::from_str(body.value()).map_err(|err| err.to_string())?;
                call.result = Some(value);
            }
        }
        Ok(())
    }

    pub fn put_image(
        &self,
        session: SessionId,
        id: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let db = self.session_db(session)?;
        let write = db.begin_write().map_err(redb_err)?;
        {
            let mut images = write.open_table(IMAGES).map_err(redb_err)?;
            images.insert(id, bytes).map_err(redb_err)?;
            let mut types = write.open_table(IMAGE_TYPES).map_err(redb_err)?;
            types.insert(id, media_type).map_err(redb_err)?;
        }
        write.commit().map_err(redb_err)?;
        let index = self.image_index()?;
        let write = index.begin_write().map_err(redb_err)?;
        {
            let mut table = write.open_table(IMAGE_INDEX).map_err(redb_err)?;
            table
                .insert(id, session.to_string().as_str())
                .map_err(redb_err)?;
        }
        write.commit().map_err(redb_err)?;
        Ok(())
    }

    pub fn get_image(&self, id: &str) -> Result<Option<(String, Vec<u8>)>, String> {
        let index = self.image_index()?;
        let session = {
            let read = index.begin_read().map_err(redb_err)?;
            let table = read.open_table(IMAGE_INDEX).map_err(redb_err)?;
            match table.get(id).map_err(redb_err)? {
                Some(session) => session.value().to_owned(),
                None => return Ok(None),
            }
        };
        let Ok(uuid) = session.parse::<uuid::Uuid>() else {
            return Ok(None);
        };
        let session = SessionId::from_uuid(uuid);
        let Some(db) = self.session_db_if_present(session)? else {
            return Ok(None);
        };
        let read = db.begin_read().map_err(redb_err)?;
        let images = read.open_table(IMAGES).map_err(redb_err)?;
        let types = read.open_table(IMAGE_TYPES).map_err(redb_err)?;
        let Some(bytes) = images.get(id).map_err(redb_err)? else {
            return Ok(None);
        };
        let Some(media_type) = types.get(id).map_err(redb_err)? else {
            return Ok(None);
        };
        Ok(Some((
            media_type.value().to_owned(),
            bytes.value().to_vec(),
        )))
    }

    /// Drop the session directory. The image index entries for it go too.
    pub fn remove_session(&self, session: SessionId) -> Result<(), String> {
        self.inner
            .dbs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&session.to_string());
        self.forget_images(session)?;
        let directory = self.inner.root.join(session.to_string());
        match std::fs::remove_dir_all(&directory) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("remove {}: {err}", directory.display())),
        }
    }

    fn forget_images(&self, session: SessionId) -> Result<(), String> {
        let index_path = self.inner.root.join("image-index.redb");
        if !index_path.exists() {
            return Ok(());
        }
        let index = self.image_index()?;
        let session = session.to_string();
        let read = index.begin_read().map_err(redb_err)?;
        let ids: Vec<String> = {
            let table = read.open_table(IMAGE_INDEX).map_err(redb_err)?;
            let mut ids = Vec::new();
            for item in table.iter().map_err(redb_err)? {
                let (id, owner) = item.map_err(redb_err)?;
                if owner.value() == session {
                    ids.push(id.value().to_owned());
                }
            }
            ids
        };
        if ids.is_empty() {
            return Ok(());
        }
        let write = index.begin_write().map_err(redb_err)?;
        {
            let mut table = write.open_table(IMAGE_INDEX).map_err(redb_err)?;
            for id in &ids {
                table.remove(id.as_str()).map_err(redb_err)?;
            }
        }
        write.commit().map_err(redb_err)?;
        Ok(())
    }

    fn session_db(&self, session: SessionId) -> Result<Arc<Database>, String> {
        let mut guard = self
            .inner
            .dbs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = session.to_string();
        if let Some(db) = guard.get(&key) {
            return Ok(Arc::clone(db));
        }
        let directory = self.inner.root.join(&key);
        std::fs::create_dir_all(&directory).map_err(|err| err.to_string())?;
        let db = Arc::new(
            Database::create(directory.join("blobs.redb")).map_err(|err| err.to_string())?,
        );
        ensure_session(&db)?;
        guard.insert(key, Arc::clone(&db));
        Ok(db)
    }

    fn session_db_if_present(&self, session: SessionId) -> Result<Option<Arc<Database>>, String> {
        let path = self.inner.root.join(session.to_string()).join("blobs.redb");
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(self.session_db(session)?))
    }

    fn image_index(&self) -> Result<Arc<Database>, String> {
        let mut guard = self
            .inner
            .image_index
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(db) = guard.as_ref() {
            return Ok(Arc::clone(db));
        }
        std::fs::create_dir_all(&self.inner.root).map_err(|err| err.to_string())?;
        let db = Arc::new(
            Database::create(self.inner.root.join("image-index.redb"))
                .map_err(|err| err.to_string())?,
        );
        let write = db.begin_write().map_err(redb_err)?;
        {
            let _ = write.open_table(IMAGE_INDEX).map_err(redb_err)?;
        }
        write.commit().map_err(redb_err)?;
        *guard = Some(Arc::clone(&db));
        Ok(db)
    }
}

fn ensure_session(db: &Database) -> Result<(), String> {
    let write = db.begin_write().map_err(redb_err)?;
    {
        let _ = write.open_table(ORIGINALS).map_err(redb_err)?;
        let _ = write.open_table(ORIGINALS_BY_SHA).map_err(redb_err)?;
        let _ = write.open_table(RESULTS).map_err(redb_err)?;
        let _ = write.open_table(IMAGES).map_err(redb_err)?;
        let _ = write.open_table(IMAGE_TYPES).map_err(redb_err)?;
    }
    write.commit().map_err(redb_err)?;
    Ok(())
}

fn result_key(message: &MessageId, call: &str) -> String {
    format!("{message}/{call}")
}

fn sha_key(sha256: &str, id: &str) -> String {
    format!("{sha256}/{id}")
}

fn is_sha_prefix(id: &str) -> bool {
    id.len() == 16 && id.chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Exclusive end of a byte-wise prefix range.
fn prefix_upper(prefix: &str) -> String {
    let mut bytes = prefix.as_bytes().to_vec();
    while let Some(last) = bytes.last_mut() {
        if *last < 0xff {
            *last += 1;
            return String::from_utf8(bytes).unwrap_or_else(|_| prefix.to_owned());
        }
        bytes.pop();
    }
    format!("{prefix}\u{ffff}")
}

fn redb_err(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// Copy blob tables and embedded tool results out of SQLite, then drop the tables.
///
/// Safe to run on every start. Once the tables are gone it does nothing.
pub async fn evacuate_sqlite(pool: &sqlx::SqlitePool, blobs: &SessionBlobs) -> Result<(), String> {
    let originals = table_exists(pool, "tool_originals").await?;
    let images = table_exists(pool, "chat_images").await?;
    if !originals && !images {
        return Ok(());
    }

    if originals {
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            r#"
            SELECT id, chat_session_id, tool_call_id, sha256, body, created_at
            FROM tool_originals
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|err| sql_timeout("evacuate tool_originals", err))?;
        for (id, session, tool_call_id, sha256, body, created_at) in rows {
            let session = parse_session(&session)?;
            let blobs = blobs.clone();
            tokio::task::spawn_blocking(move || {
                blobs.insert_original(session, &id, &tool_call_id, &sha256, &body, &created_at)
            })
            .await
            .map_err(|err| err.to_string())??;
        }
    }

    if images {
        let rows: Vec<(String, String, String, Vec<u8>)> = sqlx::query_as(
            r#"
            SELECT id, chat_session_id, media_type, bytes
            FROM chat_images
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|err| sql_timeout("evacuate chat_images", err))?;
        for (id, session, media_type, bytes) in rows {
            let session = parse_session(&session)?;
            let blobs = blobs.clone();
            tokio::task::spawn_blocking(move || blobs.put_image(session, &id, &media_type, &bytes))
                .await
                .map_err(|err| err.to_string())??;
        }
    }

    let messages: Vec<(String, String, String)> = sqlx::query_as(
        r#"
        SELECT id, chat_session_id, body
        FROM chat_messages
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|err| sql_timeout("evacuate chat_messages", err))?;
    for (id, session_id, body) in messages {
        let mut message: Message = match serde_json::from_str(&body) {
            Ok(message) => message,
            Err(_) => continue,
        };
        if message.tool_calls.iter().all(|call| call.result.is_none()) {
            continue;
        }
        let session = parse_session(&session_id)?;
        let blobs = blobs.clone();
        let stored = message.clone();
        tokio::task::spawn_blocking(move || blobs.sync_results(session, &stored))
            .await
            .map_err(|err| err.to_string())??;
        take_results(&mut message);
        let body = serde_json::to_string(&message).map_err(|err| err.to_string())?;
        sqlx::query(
            r#"
            UPDATE chat_messages
            SET body = ?1
            WHERE id = ?2 AND chat_session_id = ?3
            "#,
        )
        .bind(body)
        .bind(id)
        .bind(session_id)
        .execute(pool)
        .await
        .map_err(|err| sql_timeout("evacuate update chat_messages", err))?;
    }

    if originals {
        sqlx::query("DROP TABLE tool_originals")
            .execute(pool)
            .await
            .map_err(|err| sql_timeout("evacuate drop tool_originals", err))?;
    }
    if images {
        sqlx::query("DROP TABLE chat_images")
            .execute(pool)
            .await
            .map_err(|err| sql_timeout("evacuate drop chat_images", err))?;
    }
    Ok(())
}

async fn table_exists(pool: &sqlx::SqlitePool, name: &str) -> Result<bool, String> {
    let found: Option<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(|err| sql_timeout("table_exists", err))?;
    Ok(found.is_some())
}

fn sql_timeout(context: &'static str, err: sqlx::Error) -> String {
    crate::adapters::sqlite::log_connection_timeout(context, &err);
    err.to_string()
}

fn parse_session(id: &str) -> Result<SessionId, String> {
    let uuid = id
        .parse::<uuid::Uuid>()
        .map_err(|_| format!("session id is not a uuid: {id}"))?;
    Ok(SessionId::from_uuid(uuid))
}

/// Pull tool results off a message before it is written to SQLite.
pub fn take_results(message: &mut Message) -> Vec<(String, Value)> {
    let mut taken = Vec::new();
    for call in &mut message.tool_calls {
        if let Some(result) = call.result.take() {
            taken.push((call.id.to_string(), result));
        }
    }
    taken
}

#[cfg(test)]
mod tests {
    use robi_core::ids::{MessageId, SessionId, ToolCallId};
    use robi_core::message::{ApprovalStatus, ExecutionStatus, Message, ToolCall};
    use serde_json::json;

    use super::*;

    fn blobs() -> (SessionBlobs, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("robi-blobs-{}", uuid::Uuid::now_v7().simple()));
        let _ = std::fs::remove_dir_all(&root);
        (SessionBlobs::new(&root), root)
    }

    fn call(id: ToolCallId, result: Option<Value>) -> ToolCall {
        ToolCall {
            id,
            name: "shell".into(),
            args: json!({}),
            args_error: None,
            approval_status: ApprovalStatus::Approved,
            execution_status: ExecutionStatus::Succeeded,
            result,
            error: None,
            truncation: None,
            provider_call_id: None,
            subagent: None,
            original_id: None,
        }
    }

    #[test]
    fn an_original_round_trips_by_id_and_sha_prefix() {
        let (blobs, root) = blobs();
        let session = SessionId::new();
        let sha = "ab".repeat(32);
        blobs
            .insert_original(session, "orig-1", "call-1", &sha, "stdout", "now")
            .unwrap();
        let by_id = blobs.lookup_originals(session, "orig-1").unwrap();
        assert_eq!(by_id, vec![("orig-1".into(), "stdout".into())]);
        let by_sha = blobs.lookup_originals(session, &sha[..16]).unwrap();
        assert_eq!(by_sha, vec![("orig-1".into(), "stdout".into())]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_tool_result_is_restored_and_a_cleared_one_stays_gone() {
        let (blobs, root) = blobs();
        let session = SessionId::new();
        let call_id = ToolCallId::new();
        let mut message = Message::assistant("ran");
        message.id = MessageId::new();
        message.tool_calls = vec![call(call_id, Some(json!({"stdout": "hi"})))];
        blobs.sync_results(session, &message).unwrap();
        message.tool_calls[0].result = None;
        blobs.fill_results(session, &mut message).unwrap();
        assert_eq!(message.tool_calls[0].result, Some(json!({"stdout": "hi"})));
        message.tool_calls[0].result = None;
        blobs.sync_results(session, &message).unwrap();
        blobs.fill_results(session, &mut message).unwrap();
        assert!(message.tool_calls[0].result.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_image_round_trips_and_leaves_with_the_session() {
        let (blobs, root) = blobs();
        let session = SessionId::new();
        blobs
            .put_image(session, "img-1", "image/png", b"png")
            .unwrap();
        assert_eq!(
            blobs.get_image("img-1").unwrap(),
            Some(("image/png".into(), b"png".to_vec()))
        );
        blobs.remove_session(session).unwrap();
        assert!(blobs.get_image("img-1").unwrap().is_none());
        assert!(!root.join(session.to_string()).exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
