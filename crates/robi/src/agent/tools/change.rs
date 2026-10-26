//! What an edit tool does beyond the write itself: approval, a diff, and the
//! `file_changed` frame.
//!
//! The read-modify-write primitives live in [`super::fs_write`], because the
//! plan, todo, and docs writers use them too.

use robi_core::tool::ApprovalDecision;
use serde_json::{json, Value};

use crate::agent::review::{diff, FileDiff, FileStatus};
use crate::domain::events::EventEnvelope;

use super::context::ToolContext;

/// `NeedsApproval` when the session write rules deny `path`.
///
/// A missing or unresolvable path, or rules that do not compile, is
/// `AllowImmediately` so `execute` reports the error.
pub async fn write_approval(ctx: &ToolContext, args: &Value) -> ApprovalDecision {
    let Some(path) = args.get("path").and_then(|value| value.as_str()) else {
        return ApprovalDecision::AllowImmediately;
    };
    if path.is_empty() {
        return ApprovalDecision::AllowImmediately;
    }
    let Ok(resolved) = ctx.resolve(path) else {
        return ApprovalDecision::AllowImmediately;
    };
    match ctx.filter().await {
        Ok(filter) if filter.allows_write(&resolved.relative) => ApprovalDecision::AllowImmediately,
        Ok(_) => ApprovalDecision::NeedsApproval,
        Err(_) => ApprovalDecision::AllowImmediately,
    }
}

/// The diff of this write. `existed` is false when the file was created.
pub fn change_diff(
    path: &str,
    before: &str,
    after: &str,
    existed: bool,
    deleted: bool,
) -> FileDiff {
    let mut file_diff = diff(path, before, after);
    if deleted {
        file_diff.status = FileStatus::Deleted;
    } else if !existed {
        file_diff.status = FileStatus::Added;
    }
    file_diff
}

pub fn diff_json(file_diff: &FileDiff) -> Value {
    json!({
        "path": file_diff.path,
        "patch": file_diff.patch,
        "additions": file_diff.additions,
        "deletions": file_diff.deletions,
        "status": file_diff.status,
        "hunks": file_diff.hunks,
    })
}

/// The `file_changed` outcome word for a write's diff status.
pub fn status_outcome(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "created",
        FileStatus::Modified => "applied",
        FileStatus::Deleted => "deleted",
    }
}

/// Announce a path an edit tool just wrote.
///
/// The frame is `robi.workspace.v1.file_changed` with `source: "agent"`. A
/// context with no bus (a test) publishes nothing.
pub fn publish_file_changed(ctx: &ToolContext, path: &str, outcome: &str) {
    let Some(events) = &ctx.events else {
        return;
    };
    events.publish(EventEnvelope::file_changed(
        &ctx.workspace_id.to_string(),
        path,
        "agent",
        Some(&ctx.session_id.to_string()),
        outcome,
    ));
}
