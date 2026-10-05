//! Delete one text file.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::Value;

use super::change::{
    change_diff, diff_json, lock_path, read_text, record_baseline, write_approval,
};
use super::context::{display_path, ToolContext};

pub struct DeleteFile {
    ctx: Arc<ToolContext>,
}

impl DeleteFile {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[derive(Deserialize)]
struct DeleteArgs {
    path: String,
}

#[async_trait]
impl Tool for DeleteFile {
    fn name(&self) -> &str {
        "delete_file"
    }

    fn description(&self) -> &str {
        "Delete one text file. Refuses a directory, a missing path, and a file that is not UTF-8. Parent directories are left in place. A path the session write rules deny waits for approval on this call and does not save an allow. The result is a deletion diff."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to delete. Workspace-relative, absolute, or ~/."}
            },
            "required": ["path"],
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
        let args: DeleteArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.path.is_empty() {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        self.ctx.filter().await?;
        let resolved = self.ctx.resolve(&args.path)?;
        let _guard = lock_path(&resolved.absolute).await;
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let absolute = resolved.absolute.clone();
        let (is_dir, before) = crate::agent::blocking::call(move || {
            let is_dir = std::fs::metadata(&absolute)
                .map(|meta| meta.is_dir())
                .unwrap_or(false);
            let before = read_text(&absolute)?;
            Ok::<_, ToolError>((is_dir, before))
        })
        .await
        .map_err(ToolError::Failed)??;
        if is_dir {
            return Err(ToolError::Failed("path is a directory".into()));
        }
        let Some(before) = before else {
            return Err(ToolError::Failed("file not found".into()));
        };
        let relative = display_path(&resolved);
        let existing = self
            .ctx
            .file_changes
            .get_baseline(self.ctx.session_id, &relative)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let absolute = resolved.absolute.clone();
        let remove = || {
            let absolute = absolute.clone();
            move || {
                std::fs::remove_file(&absolute)
                    .map_err(|err| ToolError::Failed(format!("delete file: {err}")))
            }
        };
        if existing.as_ref().is_some_and(|baseline| baseline.created) {
            crate::agent::blocking::call(remove())
                .await
                .map_err(ToolError::Failed)??;
            self.ctx
                .file_changes
                .delete_baseline(self.ctx.session_id, &relative)
                .await
                .map_err(|err| ToolError::Failed(err.to_string()))?;
        } else {
            record_baseline(
                self.ctx.file_changes.as_ref(),
                self.ctx.session_id,
                &relative,
                &before,
                false,
            )
            .await?;
            crate::agent::blocking::call(remove())
                .await
                .map_err(ToolError::Failed)??;
        }
        self.ctx.note_lsp(&resolved.absolute, true).await;
        Ok(diff_json(&change_diff(&relative, &before, "", true, true)))
    }
}
