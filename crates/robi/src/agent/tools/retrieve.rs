//! Read a stored shell original back, one page at a time.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::compress::{reduce_lines, Lookup};

use super::context::ToolContext;

const RAW_CAP: usize = 32 * 1024;

pub struct Retrieve {
    ctx: Arc<ToolContext>,
}

impl Retrieve {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for Retrieve {
    fn name(&self) -> &str {
        "retrieve"
    }

    fn description(&self) -> &str {
        "Read a capped shell stream or MCP result that was folded. The <<<ROBI_LOG lines are not command output. Pass the id from a <<<ROBI_LOG header. A 16-hex sha256 from that header also works when it matches one row in this session. Pass offset from the marker's lines range when you need a folded span or a stack past the kept window. offset counts reduced lines: split on newline, then the segment after the last carriage return. raw true returns stored bytes, still capped at 32 KB, and ignores offset and limit. A header with kind=mcp is the bounded text of that MCP call, not a shell stream, and stream does not apply."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["id"],
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id from a <<<ROBI_LOG header, or the 16-hex sha256 from that header when it matches one row in this session."
                },
                "stream": {
                    "type": "string",
                    "enum": ["stdout", "stderr", "both"],
                    "default": "both",
                    "description": "Which capped stream to return. both returns stdout, then stderr, with a stderr header line between them when stderr is non-empty."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based reduced line to start at. Reduced means split on newline, then the segment after the last carriage return. Omit to start at line 1."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "default": 200,
                    "description": "Maximum reduced lines to return."
                },
                "raw": {
                    "type": "boolean",
                    "default": false,
                    "description": "True: return the stored stream bytes unchanged, still capped at 32 KB for this call, ignoring offset and limit. False: reduced lines."
                }
            }
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args: Args = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.id.is_empty() {
            return Err(ToolError::InvalidArgs("id is required".into()));
        }
        let limit = args.limit.unwrap_or(200);
        if !(1..=200).contains(&limit) {
            return Err(ToolError::InvalidArgs("limit must be from 1 to 200".into()));
        }
        let Some(store) = &self.ctx.originals else {
            return Err(ToolError::Failed("original not found".into()));
        };
        match store.lookup(self.ctx.session_id, &args.id).await {
            Ok(Lookup::Missing) => Err(ToolError::Failed("original not found".into())),
            Ok(Lookup::Ambiguous(ids)) => Err(ToolError::Failed(format!(
                "ambiguous id: {}",
                ids.join(", ")
            ))),
            Ok(Lookup::One(body)) => render(&body, &args, limit),
            Err(error) => Err(ToolError::Failed(error)),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    id: String,
    stream: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
    raw: Option<bool>,
}

fn render(body: &Value, args: &Args, limit: u32) -> Result<Value, ToolError> {
    if body.get("kind").and_then(Value::as_str) == Some("mcp") {
        return render_mcp(body, args, limit);
    }
    let stdout = body.get("stdout").and_then(Value::as_str).unwrap_or("");
    let stderr = body.get("stderr").and_then(Value::as_str).unwrap_or("");
    let exit_code = body.get("exit_code").cloned().unwrap_or(json!(0));
    let truncated = body
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let stream = args.stream.as_deref().unwrap_or("both");
    if !matches!(stream, "stdout" | "stderr" | "both") {
        return Err(ToolError::InvalidArgs(
            "stream must be stdout, stderr, or both".into(),
        ));
    }
    if args.raw.unwrap_or(false) {
        return Ok(render_raw(stdout, stderr, stream, exit_code, truncated));
    }
    let (text, header) = select_text(stdout, stderr, stream);
    let page = page_lines(&text, args.offset.unwrap_or(1), limit);
    let stdout = if stream == "stderr" {
        String::new()
    } else {
        page.text.clone()
    };
    let stderr = if stream == "stderr" {
        page.text.clone()
    } else {
        String::new()
    };
    let mut payload = json!({
        "stdout": stdout,
        "stderr": stderr,
        "exit_code": exit_code,
        "truncated": truncated,
        "start_line": page.start_line,
        "end_line": page.end_line,
        "total_lines": page.total_lines,
    });
    if stream == "both" {
        let (out, err) = split_both(&page.text, header);
        payload["stdout"] = json!(out);
        payload["stderr"] = json!(err);
    }
    if let Some(next) = page.next_offset {
        payload["next_offset"] = json!(next);
    }
    Ok(payload)
}

fn render_mcp(body: &Value, args: &Args, limit: u32) -> Result<Value, ToolError> {
    let text = body.get("text").and_then(Value::as_str).unwrap_or("");
    let truncated = body
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if args.raw.unwrap_or(false) {
        let (slice, cut, next) = cap_bytes(text, RAW_CAP);
        let mut payload = json!({
            "kind": "mcp",
            "text": slice,
            "truncated": truncated || cut,
            "start_line": 1,
            "end_line": 0,
            "total_lines": reduce_lines(text).len(),
        });
        if let Some(next) = next {
            payload["next_offset"] = json!(next);
        }
        return Ok(payload);
    }
    let page = page_lines(text, args.offset.unwrap_or(1), limit);
    let mut payload = json!({
        "kind": "mcp",
        "text": page.text,
        "truncated": truncated,
        "start_line": page.start_line,
        "end_line": page.end_line,
        "total_lines": page.total_lines,
    });
    if let Some(next) = page.next_offset {
        payload["next_offset"] = json!(next);
    }
    Ok(payload)
}

fn select_text(stdout: &str, stderr: &str, stream: &str) -> (String, bool) {
    match stream {
        "stdout" => (stdout.to_owned(), false),
        "stderr" => (stderr.to_owned(), false),
        _ => {
            if stderr.is_empty() {
                (stdout.to_owned(), false)
            } else if stdout.is_empty() {
                (format!("--- stderr ---\n{stderr}"), true)
            } else {
                (format!("{stdout}\n--- stderr ---\n{stderr}"), true)
            }
        }
    }
}

fn split_both(page: &str, header: bool) -> (String, String) {
    if !header {
        return (page.to_owned(), String::new());
    }
    match page.split_once("--- stderr ---\n") {
        Some((stdout, stderr)) => {
            let stdout = stdout.strip_suffix('\n').unwrap_or(stdout);
            (stdout.to_owned(), stderr.to_owned())
        }
        None => (page.to_owned(), String::new()),
    }
}

struct Page {
    text: String,
    start_line: usize,
    end_line: usize,
    total_lines: usize,
    next_offset: Option<usize>,
}

fn page_lines(text: &str, offset: u32, limit: u32) -> Page {
    let lines = reduce_lines(text);
    let total = lines.len();
    let start = offset.max(1) as usize;
    if start > total {
        return Page {
            text: String::new(),
            start_line: start,
            end_line: start.saturating_sub(1),
            total_lines: total,
            next_offset: None,
        };
    }
    let end = (start + limit as usize - 1).min(total);
    let text = lines[start - 1..end].join("\n");
    let next = if end < total { Some(end + 1) } else { None };
    Page {
        text,
        start_line: start,
        end_line: end,
        total_lines: total,
        next_offset: next,
    }
}

fn render_raw(
    stdout: &str,
    stderr: &str,
    stream: &str,
    exit_code: Value,
    stored_truncated: bool,
) -> Value {
    let (text, header) = select_text(stdout, stderr, stream);
    let (slice, truncated, next) = cap_bytes(&text, RAW_CAP);
    let (out, err) = if stream == "stderr" {
        (String::new(), slice.clone())
    } else if stream == "stdout" {
        (slice.clone(), String::new())
    } else {
        split_both(&slice, header)
    };
    let mut payload = json!({
        "stdout": out,
        "stderr": err,
        "exit_code": exit_code,
        "truncated": stored_truncated || truncated,
        "start_line": 1,
        "end_line": 0,
        "total_lines": reduce_lines(&text).len(),
    });
    if let Some(next) = next {
        payload["next_offset"] = json!(next);
    }
    payload
}

fn cap_bytes(text: &str, max: usize) -> (String, bool, Option<usize>) {
    if text.len() <= max {
        return (text.to_owned(), false, None);
    }
    let mut cut = max;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    (text[..cut].to_owned(), true, Some(cut))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(stream: &str) -> Args {
        Args {
            id: "id".into(),
            stream: Some(stream.into()),
            offset: None,
            limit: Some(20),
            raw: Some(false),
        }
    }

    #[test]
    fn an_mcp_row_ignores_stream_and_returns_text() {
        let body = json!({
            "kind": "mcp",
            "text": "alpha\nbeta\ngamma",
            "truncated": true,
        });
        let page = render(&body, &args("stderr"), 2).unwrap();
        assert_eq!(page["kind"], "mcp");
        assert_eq!(page["text"], "alpha\nbeta");
        assert_eq!(page["truncated"], true);
        assert_eq!(page["next_offset"], 3);
        assert!(page.get("stdout").is_none());
        assert!(page.get("stderr").is_none());
    }

    #[test]
    fn a_shell_row_still_returns_both_streams() {
        let body = json!({
            "stdout": "out",
            "stderr": "err",
            "exit_code": 1,
            "truncated": false,
        });
        let page = render(&body, &args("both"), 20).unwrap();
        assert_eq!(page["stdout"], "out");
        assert_eq!(page["stderr"], "err");
        assert_eq!(page["exit_code"], 1);
        assert!(page.get("kind").is_none());
    }
}
