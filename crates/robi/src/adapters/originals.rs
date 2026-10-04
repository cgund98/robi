//! Tool originals in the per-session blob file.
//!
//! The id still rides on the tool call. The body is not a SQLite row.

use async_trait::async_trait;
use chrono::Utc;
use robi_core::ids::SessionId;

use crate::agent::compress::{sha256_hex, Inserted, Lookup, OriginalStore};

use super::session_blobs::SessionBlobs;

pub struct BlobOriginals {
    blobs: SessionBlobs,
}

impl BlobOriginals {
    pub fn new(blobs: SessionBlobs) -> Self {
        Self { blobs }
    }
}

#[async_trait]
impl OriginalStore for BlobOriginals {
    async fn insert(
        &self,
        session: SessionId,
        tool_call_id: &str,
        body: &str,
    ) -> Result<Inserted, String> {
        let id = crate::agent::compress::new_id();
        let sha256 = sha256_hex(body);
        let created_at = Utc::now().to_rfc3339();
        let blobs = self.blobs.clone();
        let tool_call_id = tool_call_id.to_owned();
        let body = body.to_owned();
        tokio::task::spawn_blocking(move || {
            blobs.insert_original(session, &id, &tool_call_id, &sha256, &body, &created_at)?;
            Ok(Inserted { id, sha256 })
        })
        .await
        .map_err(|err| err.to_string())?
    }

    async fn lookup(&self, session: SessionId, id: &str) -> Result<Lookup, String> {
        let blobs = self.blobs.clone();
        let id = id.to_owned();
        let rows = tokio::task::spawn_blocking(move || blobs.lookup_originals(session, &id))
            .await
            .map_err(|err| err.to_string())??;
        rows_to_lookup(rows)
    }
}

fn rows_to_lookup(rows: Vec<(String, String)>) -> Result<Lookup, String> {
    match rows.len() {
        0 => Ok(Lookup::Missing),
        1 => {
            let body = serde_json::from_str(&rows[0].1).map_err(|err| err.to_string())?;
            Ok(Lookup::One(body))
        }
        _ => Ok(Lookup::Ambiguous(
            rows.into_iter().map(|(id, _)| id).collect(),
        )),
    }
}
