//! Replace an exact snippet in a text file.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::Value;

use super::change::{
    atomic_write, change_diff, diff_json, lock_path, read_text, record_baseline, write_approval,
};
use super::context::{display_path, ToolContext};
use super::marker::reject_marker;
use super::replace::apply_edit;

pub struct EditFile {
    ctx: Arc<ToolContext>,
}

impl EditFile {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    old: String,
    new: String,
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl Tool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace an exact occurrence of old with new in a text file. Copy old from read_file, including indentation. old and new must differ. An empty old is refused; use write_file to create or replace a file. More than one match is an error unless replace_all is true. A path the session write rules deny waits for approval on this call and does not save an allow. The result is the diff of this write."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to edit. Workspace-relative, absolute, or ~/."},
                "old": {"type": "string", "description": "Exact text to replace, copied from read_file."},
                "new": {"type": "string", "description": "Replacement text. Must differ from old."},
                "replace_all": {"type": "boolean", "description": "Replace every exact match. Default false."}
            },
            "required": ["path", "old", "new"],
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
        let args: EditArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.path.is_empty() {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        reject_marker(&args.old)?;
        reject_marker(&args.new)?;
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
        let Some(before) = read_text(&resolved.absolute)? else {
            return Err(ToolError::Failed("file not found".into()));
        };
        let after = apply_edit(&before, &args.old, &args.new, args.replace_all)
            .map_err(ToolError::Failed)?;
        let relative = display_path(&resolved);
        record_baseline(
            self.ctx.file_changes.as_ref(),
            self.ctx.session_id,
            &relative,
            &before,
            false,
        )
        .await?;
        atomic_write(&resolved.absolute, &after)?;
        self.ctx.note_lsp(&resolved.absolute, false).await;
        Ok(diff_json(&change_diff(
            &relative, &before, &after, true, false,
        )))
    }
}
