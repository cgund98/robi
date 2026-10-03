//! One server connection, as the tool and the tests see it.

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct ListedTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
    pub read_only: bool,
    pub destructive: bool,
    pub open_world: bool,
}

#[derive(Debug, Clone)]
pub struct Listed {
    pub tools: Vec<ListedTool>,
    /// Server instructions from initialize. Logged, never prompted.
    pub instructions: Option<String>,
    /// Display name from the server handshake. Absent when the server sent none.
    pub title: Option<String>,
    /// First `https` or `data:image` icon from the handshake.
    pub icon: Option<String>,
}

/// A connected MCP server. Implementations speak the protocol or fake it.
#[async_trait]
pub trait McpSession: Send + Sync {
    async fn list_tools(&self) -> Result<Listed, String>;

    async fn call_tool(
        &self,
        name: &str,
        args: Value,
        cancel: &CancellationToken,
    ) -> Result<Value, String>;
}

/// Opens one configured server. Tests supply a session directly.
#[async_trait]
pub trait SessionOpener: Send + Sync {
    async fn open(&self, spec: OpenSpec) -> Result<Box<dyn McpSession>, String>;
}

#[derive(Debug, Clone)]
pub struct OpenSpec {
    pub server_id: String,
    pub workspace_root: std::path::PathBuf,
    pub transport: super::config::Transport,
}
