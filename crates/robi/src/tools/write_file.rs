//! Create a file or replace its whole body.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::Value;

use super::change::{
    atomic_write, change_diff, diff_json, ensure_parent, lock_path, read_text, record_baseline,
    write_approval,
};
use super::context::{display_path, ToolContext};
use super::marker::reject_marker;

pub struct WriteFile {
    ctx: Arc<ToolContext>,
}

impl WriteFile {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create a file or replace its entire contents. path may be workspace-relative, absolute, or start with ~/. Parent directories are created. A path the session write rules deny waits for approval on this call and does not save an allow. The result is the diff of this write."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to write. Workspace-relative, absolute, or ~/."},
                "content": {"type": "string", "description": "The full new contents of the file."}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    async fn requires_approval(&self, args: &Value) -> ApprovalDecision {
        write_approval(&self.ctx, args).await
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args: WriteArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.path.is_empty() {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        reject_marker(&args.content)?;
        self.ctx.filter().await?;
        let resolved = self.ctx.resolve(&args.path)?;
        let _guard = lock_path(&resolved.absolute).await;
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if std::fs::metadata(&resolved.absolute)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
        {
            return Err(ToolError::Failed("path is a directory".into()));
        }
        let existing = read_text(&resolved.absolute)?;
        let existed = existing.is_some();
        let before = existing.unwrap_or_default();
        let relative = display_path(&resolved);
        record_baseline(
            self.ctx.file_changes.as_ref(),
            self.ctx.session_id,
            &relative,
            &before,
            !existed,
        )
        .await?;
        ensure_parent(&resolved.absolute)?;
        atomic_write(&resolved.absolute, &args.content)?;
        self.ctx.note_lsp(&resolved.absolute, false).await;
        Ok(diff_json(&change_diff(
            &relative,
            &before,
            &args.content,
            existed,
            false,
        )))
    }
}
