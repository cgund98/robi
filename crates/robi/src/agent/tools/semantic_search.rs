//! Natural-language search over the workspace index.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use robi_index::IndexState;
use serde::Deserialize;
use serde_json::{json, Value};

use super::context::{denied, ToolContext};

const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 20;
const SEARCH_K: usize = 40;
const MAX_TEXT_BYTES: usize = 32 * 1024;

pub struct SemanticSearch {
    ctx: Arc<ToolContext>,
}

impl SemanticSearch {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    query: String,
    path: Option<String>,
    limit: Option<usize>,
}

#[async_trait]
impl Tool for SemanticSearch {
    fn name(&self) -> &str {
        "semantic_search"
    }

    fn description(&self) -> &str {
        "Search the workspace by meaning. Use this for a question about behavior. Use grep when you already have the identifier or the exact string. indexing and downloading mean the corpus is incomplete, so call grep for the same question. Hits include path, line range, symbol, and text. Results stop at 8 hits or 32 KB."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "A question about what the code does."},
                "path": {"type": "string", "description": "Workspace-relative file or directory. Defaults to the workspace root."},
                "limit": {"type": "integer", "description": "How many hits to return. Default 8, maximum 20."}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args: Args = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.query.trim().is_empty() {
            return Err(ToolError::InvalidArgs("query is required".into()));
        }
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
        if limit == 0 || limit > MAX_LIMIT {
            return Err(ToolError::InvalidArgs("limit must be from 1 to 20".into()));
        }
        let prefix = match args.path.as_deref() {
            None | Some("") => None,
            Some(path) => {
                let resolved = self.ctx.resolve(path)?;
                let filter = self.ctx.filter().await?;
                if !filter.allows_read(&resolved.relative) {
                    return Err(denied(&resolved));
                }
                Some(resolved.relative)
            }
        };

        let Some(hub) = &self.ctx.index else {
            return Ok(body("indexing", 0, 0, None, Vec::new(), false, &args.query));
        };
        let session = self
            .ctx
            .sessions
            .get_chat_session(self.ctx.session_id)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let Some(index) = hub.index(session.workspace_id) else {
            let status = hub.status(session.workspace_id);
            return Ok(body(
                state_name(status.state),
                status.files_done,
                status.files_total,
                status.error.as_deref(),
                Vec::new(),
                false,
                &args.query,
            ));
        };
        let status = index.status();
        if status.state == IndexState::Downloading {
            return Ok(body(
                "downloading",
                status.files_done,
                status.files_total,
                status.error.as_deref(),
                Vec::new(),
                false,
                &args.query,
            ));
        }
        let query_vec = index
            .embed_query(&args.query)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let hits = index
            .search(&query_vec, &args.query, SEARCH_K)
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let filter = self.ctx.filter().await?;
        let mut kept = Vec::new();
        for hit in hits {
            if !filter.allows_read(&hit.path) {
                continue;
            }
            if let Some(prefix) = &prefix {
                if hit.path != *prefix && !hit.path.starts_with(&format!("{prefix}/")) {
                    continue;
                }
            }
            kept.push(hit);
            if kept.len() == limit {
                break;
            }
        }
        let (hits, truncated) = cap_text(kept);
        Ok(body(
            state_name(status.state),
            status.files_done,
            status.files_total,
            status.error.as_deref(),
            hits,
            truncated,
            &args.query,
        ))
    }
}

fn state_name(state: IndexState) -> &'static str {
    match state {
        IndexState::Downloading => "downloading",
        IndexState::Indexing => "indexing",
        IndexState::Ready => "ready",
        IndexState::Paused => "paused",
        IndexState::Failed => "failed",
    }
}

fn cap_text(hits: Vec<robi_index::ChunkHit>) -> (Vec<Value>, bool) {
    let mut used = 0usize;
    let mut truncated = false;
    let mut out = Vec::new();
    for hit in hits {
        if used >= MAX_TEXT_BYTES {
            truncated = true;
            break;
        }
        let mut text = hit.body;
        if used + text.len() > MAX_TEXT_BYTES {
            let room = MAX_TEXT_BYTES - used;
            let mut end = room.min(text.len());
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push_str("\n[truncated]");
            truncated = true;
        }
        used += text.len();
        out.push(json!({
            "path": hit.path,
            "start_line": hit.start_line,
            "end_line": hit.end_line,
            "symbol": hit.symbol,
            "language": hit.language,
            "text": text,
        }));
        if truncated {
            break;
        }
    }
    (out, truncated)
}

fn body(
    state: &str,
    files_done: u64,
    files_total: u64,
    error: Option<&str>,
    hits: Vec<Value>,
    truncated: bool,
    query: &str,
) -> Value {
    let mut value = json!({
        "query": query,
        "state": state,
        "files_done": files_done,
        "files_total": files_total,
        "truncated": truncated,
        "hits": hits,
    });
    if let Some(error) = error {
        value["error"] = json!(error);
    }
    value
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use robi_core::tool::{Tool, ToolRun};
    use robi_index::{FakeEmbedder, IndexState};
    use serde_json::json;

    use super::SemanticSearch;
    use crate::agent::index::IndexHub;
    use crate::agent::tools::apply_tests::harness;
    use crate::agent::tools::context::ToolContext;
    use crate::domain::chat_session::model::UpdateChatSessionCommand;
    use crate::domain::events::EventBus;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn drops_a_denied_hit_and_a_hit_outside_the_path() {
        let harness = harness().await;
        std::fs::write(harness.root.join("kept.rs"), "fn kept_symbol() {}\n").unwrap();
        std::fs::write(harness.root.join("other.rs"), "fn other_symbol() {}\n").unwrap();
        std::fs::create_dir_all(harness.root.join("nested")).unwrap();
        std::fs::write(
            harness.root.join("nested/hidden.rs"),
            "fn hidden_symbol() {}\n",
        )
        .unwrap();
        let session = harness
            .ctx
            .sessions
            .get_chat_session(harness.session_id)
            .await
            .unwrap();
        harness
            .ctx
            .sessions
            .update_chat_session(UpdateChatSessionCommand {
                id: harness.session_id,
                title: None,
                allow_read: None,
                allow_write: None,
                deny_read: Some(vec![r"^nested/hidden\.rs$".into()]),
                deny_write: None,
                allow_hosts: None,
                mcp_allows: None,
                mode: None,
                model_config: None,
            })
            .await
            .unwrap();
        let hub = Arc::new(IndexHub::new(
            std::env::temp_dir(),
            Arc::new(EventBus::new()),
            Arc::new(FakeEmbedder::new(4)),
        ));
        let lease = hub.acquire(session.workspace_id, harness.root.clone());
        let index = hub.index(session.workspace_id).unwrap();
        for _ in 0..100 {
            let status = index.status();
            if status.state == IndexState::Ready || status.state == IndexState::Failed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(index.status().state, IndexState::Ready);
        let ctx = Arc::new(ToolContext {
            session_id: harness.session_id,
            root: harness.ctx.root.clone(),
            sessions: Arc::clone(&harness.ctx.sessions),
            file_changes: Arc::clone(&harness.ctx.file_changes),
            index: Some(Arc::clone(&hub)),
            lsp: crate::agent::lsp::LspHub::new(),
            lsp_enabled: true,
            originals: None,
        });
        let tool = SemanticSearch::new(ctx);
        let all = tool
            .execute(json!({"query": "symbol", "limit": 8}), tool_run())
            .await
            .unwrap();
        let paths = hit_paths(&all);
        assert!(paths.contains(&"kept.rs".to_owned()), "{all}");
        assert!(paths.contains(&"other.rs".to_owned()), "{all}");
        assert!(!paths.iter().any(|path| path.contains("hidden")), "{all}");

        let scoped = tool
            .execute(json!({"query": "symbol", "path": "kept.rs"}), tool_run())
            .await
            .unwrap();
        assert_eq!(hit_paths(&scoped), vec!["kept.rs".to_owned()]);
        drop(lease);
    }

    fn tool_run() -> ToolRun {
        ToolRun::new(CancellationToken::new())
    }

    fn hit_paths(value: &serde_json::Value) -> Vec<String> {
        value["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit["path"].as_str().unwrap().to_owned())
            .collect()
    }
}
