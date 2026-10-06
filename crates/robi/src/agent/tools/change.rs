//! Shared read-modify-write for the edit tools.

use std::path::Path;

use robi_core::error::ToolError;
use robi_core::ids::SessionId;
use robi_core::tool::ApprovalDecision;
use serde_json::{json, Value};

use crate::agent::review::{diff, lock_path as lock_review_path, FileDiff, FileStatus};
use crate::domain::events::EventEnvelope;
use crate::domain::file_change::repo::FileChangeRepository;

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

pub async fn lock_path(path: &Path) -> tokio::sync::OwnedMutexGuard<()> {
    lock_review_path(path).await
}

pub fn read_text(path: &Path) -> Result<Option<String>, ToolError> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| ToolError::Failed("file is not utf-8".to_owned())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ToolError::Failed(format!("read file: {err}"))),
    }
}

pub fn ensure_parent(path: &Path) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|err| ToolError::Failed(format!("create directory: {err}")))?;
        }
    }
    Ok(())
}

pub fn atomic_write(path: &Path, text: &str) -> Result<(), ToolError> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    let tmp = path.with_file_name(format!(".{name}.robi-tmp"));
    if let Err(err) = std::fs::write(&tmp, text.as_bytes()) {
        return Err(ToolError::Failed(format!("write file: {err}")));
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(ToolError::Failed(format!("write file: {err}")));
    }
    Ok(())
}

pub async fn record_baseline(
    repo: &dyn FileChangeRepository,
    session_id: SessionId,
    path: &str,
    baseline: &str,
    created: bool,
) -> Result<(), ToolError> {
    repo.record_baseline(session_id, path, baseline, created)
        .await
        .map_err(|err| ToolError::Failed(err.to_string()))?;
    Ok(())
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

/// Announce a path an edit tool just wrote, so the docs viewer can react.
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
