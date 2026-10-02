//! Read a text file inside the workspace.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::context::{denied, display_path, ToolContext};

const MAX_READ_BYTES: usize = 32 * 1024;

pub struct ReadFile {
    ctx: Arc<ToolContext>,
}

impl ReadFile {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a text file in the workspace. path may be workspace-relative, absolute, or start with ~/. offset is the 1-based line to start at. limit is the maximum number of lines. The result includes start_line, end_line, and total_lines. When truncated is true, call again with offset set to next_offset. A path outside the workspace is refused until the session grants it. Paths denied by the session path rules are refused. A single call returns at most 32 KB."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to read. Workspace-relative, absolute, or ~/."},
                "offset": {"type": "integer", "minimum": 1, "description": "1-based line to start from."},
                "limit": {"type": "integer", "minimum": 1, "description": "Maximum number of lines to return."}
            },
            "required": ["path"],
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
        let args: ReadArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.path.is_empty() {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        let filter = self.ctx.filter().await?;
        let resolved = self.ctx.resolve(&args.path)?;
        if !filter.allows_read(&resolved.relative) {
            return Err(denied(&resolved));
        }
        read_resolved(
            &resolved.absolute,
            &display_path(&resolved),
            args.offset.unwrap_or(1),
            args.limit,
        )
    }
}

#[derive(Debug, Deserialize)]
struct ReadArgs {
    path: String,
    offset: Option<u32>,
    limit: Option<u32>,
}

fn read_resolved(
    absolute: &Path,
    display: &str,
    offset: u32,
    limit: Option<u32>,
) -> Result<Value, ToolError> {
    let metadata = fs::metadata(absolute).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ToolError::Failed(format!("file not found: {display}"))
        } else {
            ToolError::Failed(format!("read file: {err}"))
        }
    })?;
    if metadata.is_dir() {
        return Err(ToolError::Failed(format!("path is a directory: {display}")));
    }
    let bytes = fs::read(absolute).map_err(|err| ToolError::Failed(format!("read file: {err}")))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| ToolError::Failed(format!("file is not utf-8: {display}")))?;
    let window = read_window(&text, offset, limit);
    let mut payload = json!({
        "path": display,
        "content": window.content,
        "start_line": window.start_line,
        "end_line": window.end_line,
        "total_lines": window.total_lines,
        "truncated": window.next_offset.is_some(),
    });
    if let Some(next_offset) = window.next_offset {
        payload["next_offset"] = json!(next_offset);
    }
    Ok(payload)
}

struct ReadWindow {
    content: String,
    start_line: u32,
    end_line: u32,
    total_lines: u32,
    next_offset: Option<u32>,
}

fn read_window(text: &str, offset: u32, limit: Option<u32>) -> ReadWindow {
    let lines = split_keep(text);
    let total_lines = lines.len() as u32;
    let start = offset.max(1).saturating_sub(1) as usize;
    if start >= lines.len() {
        return ReadWindow {
            content: String::new(),
            start_line: offset.max(1),
            end_line: offset.max(1).saturating_sub(1),
            total_lines,
            next_offset: None,
        };
    }
    let mut end = lines.len();
    if let Some(limit) = limit.filter(|limit| *limit > 0) {
        end = end.min(start + limit as usize);
    }
    let mut content = String::new();
    let mut index = start;
    while index < end {
        let line = lines[index];
        if content.len() + line.len() > MAX_READ_BYTES {
            if content.is_empty() {
                content.push_str(truncate_bytes(line, MAX_READ_BYTES));
                index += 1;
            }
            break;
        }
        content.push_str(line);
        index += 1;
    }
    let next_offset = (index < lines.len()).then_some(index as u32 + 1);
    ReadWindow {
        content,
        start_line: start as u32 + 1,
        end_line: index as u32,
        total_lines,
        next_offset,
    }
}

fn split_keep(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return vec![""];
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            lines.push(&text[start..=index]);
            start = index + ch.len_utf8();
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn truncate_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_reports_the_next_offset() {
        let text = "one\ntwo\nthree\n";
        let window = read_window(text, 1, Some(2));
        assert_eq!(window.content, "one\ntwo\n");
        assert_eq!(window.start_line, 1);
        assert_eq!(window.end_line, 2);
        assert_eq!(window.total_lines, 3);
        assert_eq!(window.next_offset, Some(3));
    }

    #[test]
    fn a_later_offset_continues() {
        let text = "one\ntwo\nthree\n";
        let window = read_window(text, 3, None);
        assert_eq!(window.content, "three\n");
        assert!(window.next_offset.is_none());
    }
}
