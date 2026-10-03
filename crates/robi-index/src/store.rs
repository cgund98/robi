//! One sqlite-vec file for a workspace.

use std::path::Path;
use std::sync::Once;

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use crate::chunk::{Chunk, ChunkKind};
use crate::error::IndexError;
use crate::fuse::fuse;

pub const GRAMMAR_SET: &str = "1";
const SEARCH_K: usize = 40;

#[derive(Debug, Clone)]
pub struct ChunkHit {
    pub id: i64,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub symbol: String,
    pub language: String,
    pub kind: ChunkKind,
    pub body: String,
    pub score: f64,
}

pub fn install_sqlite_vec() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let raw: rusqlite::auto_extension::RawAutoExtension =
            std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const () as usize);
        if rusqlite::auto_extension::register_auto_extension(raw).is_err() {
            tracing_fallback();
        }
    });
}

fn tracing_fallback() {
    eprintln!("sqlite-vec failed to register");
}

pub fn open_connection(path: &Path) -> Result<Connection, IndexError> {
    install_sqlite_vec();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    Ok(connection)
}

pub fn ensure_schema(connection: &Connection, dimensions: usize) -> Result<(), IndexError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS files (
            path TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            language TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chunks (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            symbol TEXT NOT NULL,
            kind TEXT NOT NULL,
            body TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS chunks_path ON chunks(path);
        ",
    )?;
    connection.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            symbol,
            body,
            content = 'chunks',
            content_rowid = 'id'
        );
        CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
            INSERT INTO chunks_fts(rowid, symbol, body) VALUES (new.id, new.symbol, new.body);
        END;
        CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, symbol, body)
            VALUES ('delete', old.id, old.symbol, old.body);
        END;
        ",
    )?;
    let vec_sql = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunk_vec USING vec0(
            chunk_id INTEGER PRIMARY KEY,
            embedding float[{dimensions}]
        )"
    );
    connection.execute_batch(&vec_sql)?;
    Ok(())
}

pub fn meta_matches(
    connection: &Connection,
    model_id: &str,
    dimensions: usize,
    workspace_id: &str,
) -> Result<bool, IndexError> {
    let Some(stored_model) = meta_get(connection, "model_id")? else {
        return Ok(false);
    };
    let Some(stored_dims) = meta_get(connection, "dimensions")? else {
        return Ok(false);
    };
    let Some(stored_grammar) = meta_get(connection, "grammar_set")? else {
        return Ok(false);
    };
    let Some(stored_workspace) = meta_get(connection, "workspace_id")? else {
        return Ok(false);
    };
    Ok(stored_model == model_id
        && stored_dims == dimensions.to_string()
        && stored_grammar == GRAMMAR_SET
        && stored_workspace == workspace_id)
}

pub fn write_meta(
    connection: &Connection,
    model_id: &str,
    dimensions: usize,
    workspace_id: &str,
) -> Result<(), IndexError> {
    for (key, value) in [
        ("model_id", model_id.to_owned()),
        ("dimensions", dimensions.to_string()),
        ("grammar_set", GRAMMAR_SET.to_owned()),
        ("workspace_id", workspace_id.to_owned()),
    ] {
        connection.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    Ok(())
}

pub fn meta_get(connection: &Connection, key: &str) -> Result<Option<String>, IndexError> {
    let mut statement = connection.prepare("SELECT value FROM meta WHERE key = ?1")?;
    let mut rows = statement.query(params![key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn set_paused(connection: &Connection, paused: bool) -> Result<(), IndexError> {
    connection.execute(
        "INSERT INTO meta(key, value) VALUES ('paused', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if paused { "1" } else { "0" }],
    )?;
    Ok(())
}

pub fn is_paused(connection: &Connection) -> Result<bool, IndexError> {
    Ok(meta_get(connection, "paused")?.as_deref() == Some("1"))
}

pub fn content_hash(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

pub fn stored_hash(connection: &Connection, path: &str) -> Result<Option<String>, IndexError> {
    let mut statement = connection.prepare("SELECT content_hash FROM files WHERE path = ?1")?;
    let mut rows = statement.query(params![path])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn replace_file(
    connection: &Connection,
    path: &str,
    hash: &str,
    language: &str,
    chunks: &[Chunk],
    vectors: &[Vec<f32>],
) -> Result<(), IndexError> {
    if chunks.len() != vectors.len() {
        return Err(IndexError::Message("chunk and vector counts differ".into()));
    }
    let tx = connection.unchecked_transaction()?;
    delete_path_in(&tx, path)?;
    tx.execute(
        "INSERT INTO files(path, content_hash, language) VALUES (?1, ?2, ?3)",
        params![path, hash, language],
    )?;
    for (chunk, vector) in chunks.iter().zip(vectors) {
        tx.execute(
            "INSERT INTO chunks(path, start_line, end_line, symbol, kind, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                path,
                chunk.start_line,
                chunk.end_line,
                chunk.symbol,
                chunk.kind.as_str(),
                chunk.body,
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO chunk_vec(chunk_id, embedding) VALUES (?1, ?2)",
            params![id, f32s_to_bytes(vector)],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn delete_path(connection: &Connection, path: &str) -> Result<(), IndexError> {
    let tx = connection.unchecked_transaction()?;
    delete_path_in(&tx, path)?;
    tx.commit()?;
    Ok(())
}

fn delete_path_in(connection: &Connection, path: &str) -> Result<(), IndexError> {
    let prefix = format!("{path}/%");
    connection.execute(
        "DELETE FROM chunk_vec WHERE chunk_id IN (
            SELECT id FROM chunks WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'
        )",
        params![path, escape_like(&prefix)],
    )?;
    connection.execute(
        "DELETE FROM chunks WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
        params![path, escape_like(&prefix)],
    )?;
    connection.execute(
        "DELETE FROM files WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
        params![path, escape_like(&prefix)],
    )?;
    Ok(())
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn search(
    connection: &Connection,
    query_vec: &[f32],
    fts: &str,
    limit: usize,
) -> Result<Vec<ChunkHit>, IndexError> {
    let vector_hits = vector_search(connection, query_vec)?;
    let fts_hits = fts_search(connection, fts)
        .unwrap_or_else(|_| fts_search(connection, &quote_phrase(fts)).unwrap_or_default());
    let mut fused = fuse(&vector_hits, &fts_hits);
    fused.truncate(limit);
    Ok(fused)
}

fn vector_search(connection: &Connection, query_vec: &[f32]) -> Result<Vec<ChunkHit>, IndexError> {
    let mut statement = connection.prepare(
        "SELECT chunk_id, distance FROM chunk_vec
         WHERE embedding MATCH ?1 AND k = ?2
         ORDER BY distance",
    )?;
    let ids: Vec<i64> = statement
        .query_map(params![f32s_to_bytes(query_vec), SEARCH_K as i64], |row| {
            row.get(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    load_hits(connection, &ids)
}

fn fts_search(connection: &Connection, query: &str) -> Result<Vec<ChunkHit>, IndexError> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare("SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY rank LIMIT ?2")?;
    let ids: Vec<i64> = statement
        .query_map(params![query, SEARCH_K as i64], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    load_hits(connection, &ids)
}

fn load_hits(connection: &Connection, ids: &[i64]) -> Result<Vec<ChunkHit>, IndexError> {
    let mut hits = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(hit) = load_hit(connection, *id)? else {
            continue;
        };
        hits.push(hit);
    }
    Ok(hits)
}

fn load_hit(connection: &Connection, id: i64) -> Result<Option<ChunkHit>, IndexError> {
    let mut statement = connection.prepare(
        "SELECT c.path, c.start_line, c.end_line, c.symbol, c.kind, c.body, f.language
         FROM chunks c
         JOIN files f ON f.path = c.path
         WHERE c.id = ?1",
    )?;
    let mut rows = statement.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let kind = match row.get::<_, String>(4)?.as_str() {
        "window" => ChunkKind::Window,
        _ => ChunkKind::Symbol,
    };
    Ok(Some(ChunkHit {
        id,
        path: row.get(0)?,
        start_line: row.get(1)?,
        end_line: row.get(2)?,
        symbol: row.get(3)?,
        kind,
        body: row.get(5)?,
        language: row.get(6)?,
        score: 0.0,
    }))
}

fn quote_phrase(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}

fn f32s_to_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub fn remove_db_files(path: &Path) {
    let display = path.display().to_string();
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{display}-wal"));
    let _ = std::fs::remove_file(format!("{display}-shm"));
}
