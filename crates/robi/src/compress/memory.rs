//! In-memory originals for tests and handlers that have no database.

use std::sync::Mutex;

use super::store::{Inserted, Lookup, OriginalStore};
use super::{new_id, sha256_hex};
use async_trait::async_trait;
use robi_core::ids::SessionId;

#[derive(Default)]
pub struct MemoryOriginals {
    rows: Mutex<Vec<Row>>,
}

struct Row {
    id: String,
    session: String,
    sha256: String,
    body: String,
}

#[async_trait]
impl OriginalStore for MemoryOriginals {
    async fn insert(
        &self,
        session: SessionId,
        _tool_call_id: &str,
        body: &str,
    ) -> Result<Inserted, String> {
        let id = new_id();
        let sha256 = sha256_hex(body);
        self.rows.lock().map_err(|err| err.to_string())?.push(Row {
            id: id.clone(),
            session: session.to_string(),
            sha256: sha256.clone(),
            body: body.to_owned(),
        });
        Ok(Inserted { id, sha256 })
    }

    async fn lookup(&self, session: SessionId, id: &str) -> Result<Lookup, String> {
        let rows = self.rows.lock().map_err(|err| err.to_string())?;
        let session = session.to_string();
        let matched: Vec<_> = if id.len() == 16 && id.chars().all(|ch| ch.is_ascii_hexdigit()) {
            rows.iter()
                .filter(|row| row.session == session && row.sha256.starts_with(id))
                .collect()
        } else {
            rows.iter()
                .filter(|row| row.session == session && row.id == id)
                .collect()
        };
        match matched.len() {
            0 => Ok(Lookup::Missing),
            1 => Ok(Lookup::One(
                serde_json::from_str::<serde_json::Value>(&matched[0].body)
                    .map_err(|err| err.to_string())?,
            )),
            _ => Ok(Lookup::Ambiguous(
                matched.iter().map(|row| row.id.clone()).collect(),
            )),
        }
    }
}
