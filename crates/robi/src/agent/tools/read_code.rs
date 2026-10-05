//! Outline a source file, or return an exact window when compression is off.

use std::fs;
use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use robi_index::{language_for_path, outline, Language};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::context::{denied, display_path, ToolContext};
use super::read_file::read_window;

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const BINARY_PROBE: usize = 8 * 1024;

const DESCRIPTION: &str = "\
read_code returns an outline of a source file. Signatures are real source. \
A body is real source only when you pass it in focus_symbols. Every other \
body is one <<<ROBI_OMITTED ...>>> line. That line is not in the file.\n\n\
Use compress true (the default) to see a large file. Pass focus_symbols for \
the function, method, or struct you are about to reason about or edit. \
Symbols use the same names as semantic_search: PaymentService::process_card \
in Rust and Go, PaymentService.process_card in TypeScript and Python. A \
bare name is accepted when it matches one symbol in the file.\n\n\
Quote edit_file's old and new only from read_file, or from a read_code call \
with compress false. Never copy a ROBI_OMITTED line into old, new, or \
write_file content. To edit a folded body, call read_code again with that \
symbol in focus_symbols, or read_file with offset set to the marker's \
start line. depth 0 is the file map. depth 1 is the default. depth 2 keeps \
top-level function bodies and is rarely worth it. expand_imports true only \
when the bug is in the imports.";

pub struct ReadCode {
    ctx: Arc<ToolContext>,
}

impl ReadCode {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    compress: Option<bool>,
    focus_symbols: Option<Vec<String>>,
    expand_imports: Option<bool>,
    depth: Option<u8>,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[async_trait]
impl Tool for ReadCode {
    fn name(&self) -> &str {
        "read_code"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File to read. Workspace-relative, absolute, or starting with ~/."
                },
                "compress": {
                    "type": "boolean",
                    "default": true,
                    "description": "True: outline. Signatures are source slices; bodies not in focus_symbols are markers. False: exact source, same 32 KB window as read_file, no markers. Omit this, or leave it true, unless you need bytes to quote in edit_file."
                },
                "focus_symbols": {
                    "type": "array",
                    "maxItems": 8,
                    "items": {"type": "string", "minLength": 1, "maxLength": 200},
                    "description": "Qualified symbols whose bodies are copied in full. Rust and Go use '::'. Other languages use '.'. Example: [\"PaymentService::process_card\", \"validate_expiry\"]. A name that hits more than one symbol unfolds none of those hits. Ignored unless compress is true; sending it with compress false is an error."
                },
                "expand_imports": {
                    "type": "boolean",
                    "default": false,
                    "description": "False: a leading import or use run longer than 15 lines becomes one marker. False does not fold a later import. True: that run is copied through."
                },
                "depth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2,
                    "default": 1,
                    "description": "How much of an unfocused file stays open. 0: top-level items collapse to a signature plus one marker, including impls and classes. 1: impls, classes, traits, and modules stay open; function and method bodies collapse; struct and enum field lists collapse. 2: top-level function bodies stay open too; only nested function bodies collapse. A focused symbol is expanded in full at every depth, and containers on the path to it stay open. Ignored when compress is false."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based line to start from. Only valid when compress is false."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of lines. Only valid when compress is false."
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
        let compress = args.compress.unwrap_or(true);
        validate(&args, compress)?;
        if args.path.is_empty() {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        let filter = self.ctx.filter().await?;
        let resolved = self.ctx.resolve(&args.path)?;
        if !filter.allows_read(&resolved.relative) {
            return Err(denied(&resolved));
        }
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let display = display_path(&resolved);
        let absolute = resolved.absolute.clone();
        let display_for_read = display.clone();
        let text = crate::agent::blocking::call(move || read_text(&absolute, &display_for_read))
            .await
            .map_err(ToolError::Failed)??;
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let language =
            language_for_path(&display).filter(|language| *language != Language::Markdown);
        if !compress || language.is_none() {
            let window = read_window(&text, args.offset.unwrap_or(1), args.limit);
            let mut payload = json!({
                "view": "source",
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
            if language.is_none() && compress {
                payload["reason"] = json!("no grammar");
            }
            return Ok(payload);
        }
        let language = language.expect("grammar");
        let focus = args.focus_symbols.unwrap_or_default();
        let depth = args.depth.unwrap_or(1);
        let rendered = outline(
            &text,
            language,
            &focus,
            depth,
            args.expand_imports.unwrap_or(false),
        )
        .map_err(|err| ToolError::Failed(err.message))?;
        let mut payload = json!({
            "view": "outline",
            "path": display,
            "language": language.as_str(),
            "file_sha256": hex_sha256(text.as_bytes()),
            "content": rendered.content,
            "total_lines": rendered.total_lines,
            "bytes": rendered.content.len(),
            "truncated": rendered.truncated,
            "depth_applied": rendered.depth_applied,
            "focused": rendered.focused,
            "omitted": rendered.omitted.iter().map(|item| json!({
                "symbol": item.symbol,
                "start_line": item.start_line,
                "end_line": item.end_line,
                "sha256": item.sha256,
            })).collect::<Vec<_>>(),
            "missing": rendered.missing,
            "ambiguous": rendered.ambiguous.iter().map(|item| json!({
                "focus": item.focus,
                "candidates": item.candidates.iter().map(|candidate| json!({
                    "symbol": candidate.symbol,
                    "start_line": candidate.start_line,
                    "end_line": candidate.end_line,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        if let Some(hint) = rendered.hint {
            payload["hint"] = json!(hint);
        }
        Ok(payload)
    }
}

fn validate(args: &Args, compress: bool) -> Result<(), ToolError> {
    if compress {
        if args.offset.is_some() || args.limit.is_some() {
            return Err(ToolError::InvalidArgs("invalid arguments".into()));
        }
        if let Some(depth) = args.depth {
            if depth > 2 {
                return Err(ToolError::InvalidArgs("invalid arguments".into()));
            }
        }
        if let Some(focus) = &args.focus_symbols {
            if focus.len() > 8 {
                return Err(ToolError::InvalidArgs("invalid arguments".into()));
            }
            for symbol in focus {
                if symbol.is_empty() || symbol.len() > 200 {
                    return Err(ToolError::InvalidArgs("invalid arguments".into()));
                }
            }
        }
        return Ok(());
    }
    if args.focus_symbols.is_some() || args.expand_imports.is_some() || args.depth.is_some() {
        return Err(ToolError::InvalidArgs("invalid arguments".into()));
    }
    Ok(())
}

fn read_text(absolute: &std::path::Path, display: &str) -> Result<String, ToolError> {
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
    if metadata.len() > MAX_FILE_BYTES {
        return Err(ToolError::Failed(
            "file is too large; use read_file with offset and limit".into(),
        ));
    }
    let bytes = fs::read(absolute).map_err(|err| ToolError::Failed(format!("read file: {err}")))?;
    let probe = bytes.len().min(BINARY_PROBE);
    if bytes[..probe].contains(&0) {
        return Err(ToolError::Failed("file is binary".into()));
    }
    String::from_utf8(bytes).map_err(|_| ToolError::Failed(format!("file is not utf-8: {display}")))
}

fn hex_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}
