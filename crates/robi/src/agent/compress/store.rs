//! The session row a compressed shell result points at.

use async_trait::async_trait;
use robi_core::ids::SessionId;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Inserted {
    pub id: String,
    pub sha256: String,
}

#[derive(Debug)]
pub enum Lookup {
    One(Value),
    Missing,
    Ambiguous(Vec<String>),
}

#[async_trait]
pub trait OriginalStore: Send + Sync {
    async fn insert(
        &self,
        session: SessionId,
        tool_call_id: &str,
        body: &str,
    ) -> Result<Inserted, String>;

    async fn lookup(&self, session: SessionId, id: &str) -> Result<Lookup, String>;
}
