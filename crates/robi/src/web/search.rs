//! The search the `web_search` tool calls. Brave is the production engine.

use async_trait::async_trait;

/// One public result. The snippet is untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// A search that failed before or while calling the engine.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SearchError(pub String);

/// One query in, up to five hits out. The tool does not choose the engine.
#[async_trait]
pub trait SearchEngine: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<SearchHit>, SearchError>;
}
