//! List one directory, omitting paths the session may not read.

use std::fs;
use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::context::{denied, display_path, ToolContext};

pub struct ListDir {
    ctx: Arc<ToolContext>,
}

impl ListDir {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for ListDir {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List one directory. path may be workspace-relative, outside the workspace (../), absolute, or start with ~/. Omit path to list the workspace root. A path outside the workspace is refused until the session grants it. Children denied by the session path rules are omitted. This does not recurse and does not apply .gitignore."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory to list. Workspace-relative, absolute, or ~/. Defaults to the workspace root."}
            },
            "additionalProperties": false
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, cancel: CancellationToken) -> Result<Value, ToolError> {
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args: ListArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        let filter = self.ctx.filter().await?;
        let argument = args.path.unwrap_or_default();
        let resolved = self.ctx.resolve(&argument)?;
        if !filter.allows_read(&resolved.relative) {
            return Err(denied(&resolved));
        }
        let metadata = fs::metadata(&resolved.absolute)
            .map_err(|err| ToolError::Failed(format!("list directory: {err}")))?;
        if !metadata.is_dir() {
            return Err(ToolError::Failed(format!(
                "path is not a directory: {}",
                display_path(&resolved)
            )));
        }
        let mut entries = Vec::new();
        let read = fs::read_dir(&resolved.absolute)
            .map_err(|err| ToolError::Failed(format!("list directory: {err}")))?;
        for entry in read {
            let entry = entry.map_err(|err| ToolError::Failed(format!("list directory: {err}")))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let relative = if resolved.relative.is_empty() {
                name.clone()
            } else {
                format!("{}/{name}", resolved.relative)
            };
            if !filter.allows_read(&relative) {
                continue;
            }
            let kind = entry
                .file_type()
                .map(|kind| {
                    if kind.is_symlink() {
                        "symlink"
                    } else if kind.is_dir() {
                        "dir"
                    } else {
                        "file"
                    }
                })
                .unwrap_or("file");
            entries.push(json!({"name": name, "kind": kind}));
        }
        entries.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
        Ok(json!({
            "path": display_path(&resolved),
            "entries": entries,
        }))
    }
}

#[derive(Debug, Deserialize)]
struct ListArgs {
    path: Option<String>,
}
