//! Replace an exact snippet in a text file.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::Value;

use super::change::{change_diff, diff_json, publish_file_changed, status_outcome, write_approval};
use super::context::{display_path, ToolContext};
use super::fs_write::{atomic_write, lock_path, read_text, record_baseline};
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
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl Tool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace an exact occurrence of old_string with new_string in a text file. Copy old_string from read_file, including indentation. old_string and new_string must differ. An empty old_string is refused; use write_file to create or replace a file. More than one match is an error unless replace_all is true. A path the session write rules deny waits for approval on this call and does not save an allow. The result is the diff of this write."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to edit. Workspace-relative, absolute, or ~/."},
                "old_string": {"type": "string", "description": "Exact text to replace, copied from read_file."},
                "new_string": {"type": "string", "description": "Replacement text. Must differ from old_string. Send an empty string to delete the match."},
                "replace_all": {"type": "boolean", "description": "Replace every exact match. Default false."}
            },
            "required": ["path", "old_string", "new_string"],
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
        let args: EditArgs = super::args::parse_args(args, &["path", "old_string", "new_string"])?;
        if args.path.is_empty() {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        reject_marker(&args.old_string)?;
        reject_marker(&args.new_string)?;
        self.ctx.filter().await?;
        let resolved = self.ctx.resolve(&args.path)?;
        let _guard = lock_path(&resolved.absolute).await?;
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
        let after = apply_edit(
            &before,
            &args.old_string,
            &args.new_string,
            args.replace_all,
        )
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
        let absolute = resolved.absolute.clone();
        let written = after.clone();
        crate::agent::blocking::call(move || atomic_write(&absolute, &written))
            .await
            .map_err(ToolError::Failed)??;
        let file_diff = change_diff(&relative, &before, &after, true, false);
        publish_file_changed(&self.ctx, &relative, status_outcome(file_diff.status));
        Ok(diff_json(&file_diff))
    }
}
