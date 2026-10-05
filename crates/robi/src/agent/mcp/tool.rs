//! One remote tool, registered as `mcp_<server>_<tool>`.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::content::render_owned;
use super::session::McpSession;

#[derive(Debug, Clone, Default)]
pub struct Hints {
    pub read_only: bool,
    pub destructive: bool,
    pub open_world: bool,
}

pub struct McpTool {
    name: String,
    description: String,
    parameters: Value,
    server: String,
    remote: String,
    pub hints: Hints,
    session: Arc<dyn McpSession>,
    /// One `tools/call` at a time for this server. Shared by every tool on it.
    gate: Arc<Mutex<()>>,
    allows: Arc<dyn AllowList>,
}

/// Session allow list. Side-effect free on read.
#[async_trait]
pub trait AllowList: Send + Sync {
    async fn allows(&self, server: &str, tool: &str) -> bool;
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        if self.allows.allows(&self.server, &self.remote).await {
            ApprovalDecision::AllowImmediately
        } else {
            ApprovalDecision::NeedsApproval
        }
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if !args.is_object() {
            return Err(ToolError::InvalidArgs(
                "arguments must be a JSON object".into(),
            ));
        }
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let _permit = self.gate.lock().await;
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        tracing::info!(server = %self.server, tool = %self.remote, "waiting on mcp tool");
        let result = self
            .session
            .call_tool(&self.remote, args, &run.cancel)
            .await
            .map_err(|err| {
                if err == "cancelled" {
                    ToolError::Cancelled
                } else if err == "timeout" {
                    ToolError::Failed("timeout".into())
                } else {
                    ToolError::Failed(err)
                }
            })?;
        if result.get("task").is_some() && result.get("content").is_none() {
            return Err(ToolError::Failed(
                "a task handle is not a tool result".into(),
            ));
        }
        match render_owned(&result) {
            Ok(text) => {
                tracing::info!(
                    server = %self.server,
                    tool = %self.remote,
                    bytes = text.len(),
                    "mcp tool result"
                );
                Ok(json!(text))
            }
            Err(message) => Err(ToolError::Failed(message)),
        }
    }
}

/// The session, call gate, and allow list shared by every tool on one server.
pub struct McpRuntime {
    pub session: Arc<dyn McpSession>,
    pub gate: Arc<Mutex<()>>,
    pub allows: Arc<dyn AllowList>,
}

impl McpTool {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
        server: impl Into<String>,
        remote: impl Into<String>,
        hints: Hints,
        runtime: McpRuntime,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
            server: server.into(),
            remote: remote.into(),
            hints,
            session: runtime.session,
            gate: runtime.gate,
            allows: runtime.allows,
        }
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    pub fn remote(&self) -> &str {
        &self.remote
    }
}
