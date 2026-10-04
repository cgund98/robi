//! Language-server tools. A missing server is a result, not a failed turn.

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::lsp::{
    diagnostic_items, location_hits, scalar_column, symbol_kind_name, to_lsp, LspClient, LspError,
    PositionError, SyncedDoc, Unavailable, MAX_FILE_BYTES,
};
use crate::agent::workspace::PathFilter;

use super::context::{denied, display_path, ToolContext};

const DEFINITION_LIMIT: usize = 20;
const REFERENCE_LIMIT: usize = 50;
const SYMBOL_LIMIT: usize = 50;
const DIAGNOSTIC_LIMIT: usize = 40;

pub struct Diagnostics {
    ctx: Arc<ToolContext>,
}

pub struct Definition {
    ctx: Arc<ToolContext>,
}

pub struct References {
    ctx: Arc<ToolContext>,
}

pub struct Hover {
    ctx: Arc<ToolContext>,
}

pub struct WorkspaceSymbol {
    ctx: Arc<ToolContext>,
}

impl Diagnostics {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Definition {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl References {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl Hover {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

impl WorkspaceSymbol {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for Diagnostics {
    fn name(&self) -> &str {
        "diagnostics"
    }

    fn description(&self) -> &str {
        "Diagnostics for one source file from its language server. path is workspace-relative, absolute, or ~/. Call this after editing a supported file. available false means no server is installed or the language is unsupported; use the shell or grep. pending true means the check is still running, so call once more. Errors and warnings only."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to check. Workspace-relative, absolute, or ~/."}
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

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        let path = required_path(args)?;
        let prepared = prepare_file(&self.ctx, &path, None, &run).await?;
        let Prepared::Ready(ready) = prepared else {
            return Ok(prepared.unavailable());
        };
        let outcome = ready
            .client
            .diagnostics(
                &ready.synced.uri,
                ready.synced.version,
                ready.synced.synced_at,
                &run.cancel,
            )
            .await;
        match outcome {
            Err(LspError::Cancelled) => Err(ToolError::Cancelled),
            Ok((items, pending)) => {
                let (diagnostics, truncated) = diagnostic_items(
                    &items,
                    &ready.absolute,
                    &self.ctx.root,
                    &ready.filter,
                    DIAGNOSTIC_LIMIT,
                );
                Ok(json!({
                    "available": true,
                    "language": ready.language,
                    "server": ready.server,
                    "path": ready.display,
                    "pending": pending,
                    "truncated": truncated,
                    "diagnostics": diagnostics,
                }))
            }
            Err(err) => Ok(from_error(&err, Some(ready.language), Some(ready.server))),
        }
    }
}

#[async_trait]
impl Tool for Definition {
    fn name(&self) -> &str {
        "definition"
    }

    fn description(&self) -> &str {
        "Go to the definition of the symbol at a 1-based line and character. Use this for a symbol. Use grep for an exact string. available false means the language server cannot answer; use grep."
    }

    fn parameters(&self) -> Value {
        position_schema("File that contains the symbol.")
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        locate(&self.ctx, args, &run, LocateKind::Definition).await
    }
}

#[async_trait]
impl Tool for References {
    fn name(&self) -> &str {
        "references"
    }

    fn description(&self) -> &str {
        "Find references to the symbol at a 1-based line and character, including the declaration. available false means the language server cannot answer; use grep."
    }

    fn parameters(&self) -> Value {
        position_schema("File that contains the symbol.")
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        locate(&self.ctx, args, &run, LocateKind::References).await
    }
}

#[async_trait]
impl Tool for Hover {
    fn name(&self) -> &str {
        "hover"
    }

    fn description(&self) -> &str {
        "Type and documentation for the symbol at a 1-based line and character, as plaintext. available false means the language server cannot answer."
    }

    fn parameters(&self) -> Value {
        position_schema("File that contains the symbol.")
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        let args = position_args(args)?;
        let prepared = prepare_file(
            &self.ctx,
            &args.path,
            Some((args.line, args.character)),
            &run,
        )
        .await?;
        let Prepared::Ready(ready) = prepared else {
            return Ok(prepared.unavailable());
        };
        let position = to_lsp(&ready.text, args.line, args.character).map_err(position_error)?;
        match ready
            .client
            .hover(&ready.synced.uri, position, &run.cancel)
            .await
        {
            Ok(contents) => Ok(json!({
                "available": true,
                "language": ready.language,
                "server": ready.server,
                "path": ready.display,
                "contents": contents,
            })),
            Err(LspError::Cancelled) => Err(ToolError::Cancelled),
            Err(err) => Ok(from_error(&err, Some(ready.language), Some(ready.server))),
        }
    }
}

#[async_trait]
impl Tool for WorkspaceSymbol {
    fn name(&self) -> &str {
        "workspace_symbol"
    }

    fn description(&self) -> &str {
        "Find a symbol by name across the workspace. query is a non-empty name or prefix. Use this when you know the name and not the file. available false means no language server is installed for a language in this workspace; use grep."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Symbol name or prefix. Must not be empty."}
            },
            "required": ["query"],
            "additionalProperties": false
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
        let args: QueryArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.query.trim().is_empty() {
            return Err(ToolError::InvalidArgs("query is empty".into()));
        }
        let filter = self.ctx.filter().await?;
        let present = self.ctx.lsp.present(&self.ctx.root, &filter);
        if present.is_empty() {
            tracing::warn!("workspace symbol search found no supported language");
            return Ok(unavailable(
                "unsupported",
                None,
                None,
                "No supported language in this workspace. Use grep.",
            ));
        }
        let installed: Vec<_> = present
            .iter()
            .filter_map(|(spec, installed)| installed.then_some(*spec))
            .collect();
        if installed.is_empty() {
            let hint = present
                .first()
                .map(|(spec, _)| {
                    format!(
                        "{} is not on PATH. Use the shell to compile, or grep.",
                        spec.argv[0]
                    )
                })
                .unwrap_or_else(|| "No language server is on PATH. Use grep.".into());
            tracing::warn!("workspace symbol search found no language server on PATH");
            return Ok(unavailable("no_server", None, None, &hint));
        }
        let mut symbols = Vec::new();
        let mut any = false;
        let mut timed_out = true;
        for spec in installed {
            if run.cancel.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            let client = match self.ctx.lsp.client(&self.ctx.root, spec).await {
                Ok(client) => client,
                Err(Unavailable::NoServer) => continue,
                Err(Unavailable::Timeout) => continue,
                Err(Unavailable::ServerFailed(_)) => continue,
            };
            match client.symbols(&args.query, &run.cancel).await {
                Ok(hits) => {
                    any = true;
                    timed_out = false;
                    for hit in hits {
                        let relative = crate::agent::workspace::workspace_relative(
                            &self.ctx.root,
                            &hit.absolute,
                        );
                        if relative == ".."
                            || relative.starts_with("../")
                            || !filter.allows_read(&relative)
                        {
                            continue;
                        }
                        symbols.push(json!({
                            "name": hit.name,
                            "kind": symbol_kind_name(hit.kind),
                            "path": relative,
                            "line": hit.line + 1,
                            "character": scalar_column(&hit.absolute, hit.line, hit.character),
                        }));
                    }
                }
                Err(LspError::Cancelled) => return Err(ToolError::Cancelled),
                Err(LspError::Timeout) => {}
                Err(LspError::Stopped) | Err(LspError::Protocol(_)) => {}
            }
        }
        if !any {
            let reason = if timed_out {
                "timeout"
            } else {
                "server_failed"
            };
            tracing::warn!(reason, "workspace symbol search failed");
            return Ok(unavailable(
                reason,
                None,
                None,
                "The language server did not answer. Call again, or use grep.",
            ));
        }
        tracing::info!(count = symbols.len(), "workspace symbol search succeeded");
        symbols.sort_by(|left, right| {
            let left_name = left.get("name").and_then(Value::as_str).unwrap_or("");
            let right_name = right.get("name").and_then(Value::as_str).unwrap_or("");
            let left_path = left.get("path").and_then(Value::as_str).unwrap_or("");
            let right_path = right.get("path").and_then(Value::as_str).unwrap_or("");
            left_name.cmp(right_name).then(left_path.cmp(right_path))
        });
        let truncated = symbols.len() > SYMBOL_LIMIT;
        symbols.truncate(SYMBOL_LIMIT);
        Ok(json!({
            "available": true,
            "truncated": truncated,
            "symbols": symbols,
        }))
    }
}

struct Ready {
    client: Arc<LspClient>,
    filter: PathFilter,
    absolute: std::path::PathBuf,
    display: String,
    language: &'static str,
    server: &'static str,
    text: String,
    synced: SyncedDoc,
}

enum Prepared {
    Unavailable(Value),
    Ready(Box<Ready>),
}

impl Prepared {
    fn unavailable(self) -> Value {
        match self {
            Prepared::Unavailable(value) => value,
            Prepared::Ready(_) => json!({ "available": false }),
        }
    }
}

async fn prepare_file(
    ctx: &ToolContext,
    path: &str,
    position: Option<(u32, u32)>,
    run: &ToolRun,
) -> Result<Prepared, ToolError> {
    if run.cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    let filter = ctx.filter().await?;
    let resolved = ctx.resolve(path)?;
    if resolved.relative == ".." || resolved.relative.starts_with("../") {
        return Err(ToolError::Failed("path is outside the workspace".into()));
    }
    if !filter.allows_read(&resolved.relative) {
        return Err(denied(&resolved));
    }
    let Some(choice) = ctx.lsp.server_for(&resolved.absolute) else {
        tracing::warn!(
            path = %resolved.relative,
            "no language server for this file"
        );
        return Ok(Prepared::Unavailable(unavailable(
            "unsupported",
            None,
            None,
            "No language server is registered for this file. Use grep or the shell.",
        )));
    };
    let display = display_path(&resolved);
    let meta = match std::fs::metadata(&resolved.absolute) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ToolError::Failed(format!("file not found: {display}")));
        }
        Err(err) => return Err(ToolError::Failed(format!("read file: {err}"))),
    };
    if meta.is_dir() {
        return Err(ToolError::Failed(format!("path is a directory: {display}")));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(ToolError::Failed(format!("file is too large: {display}")));
    }
    let bytes = std::fs::read(&resolved.absolute)
        .map_err(|err| ToolError::Failed(format!("read file: {err}")))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| ToolError::Failed(format!("file is not utf-8: {display}")))?;
    if let Some((line, character)) = position {
        to_lsp(&text, line, character).map_err(position_error)?;
    }
    let client = match ctx.lsp.client(&ctx.root, choice.spec).await {
        Ok(client) => client,
        Err(reason) => {
            return Ok(Prepared::Unavailable(unavailable(
                reason_name(&reason),
                Some(choice.language),
                Some(choice.spec.id),
                &hint(&reason, choice.spec.argv[0]),
            )));
        }
    };
    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let synced = client
        .sync(
            &resolved.absolute,
            choice.language,
            &text,
            modified,
            meta.len(),
        )
        .await
        .map_err(|err| match err {
            LspError::Cancelled => ToolError::Cancelled,
            other => ToolError::Failed(other.to_string()),
        })?;
    Ok(Prepared::Ready(Box::new(Ready {
        client,
        filter,
        absolute: resolved.absolute,
        display,
        language: choice.language,
        server: choice.spec.id,
        text,
        synced,
    })))
}

enum LocateKind {
    Definition,
    References,
}

async fn locate(
    ctx: &ToolContext,
    args: Value,
    run: &ToolRun,
    kind: LocateKind,
) -> Result<Value, ToolError> {
    let limit = match kind {
        LocateKind::Definition => DEFINITION_LIMIT,
        LocateKind::References => REFERENCE_LIMIT,
    };
    let args = position_args(args)?;
    let prepared = prepare_file(ctx, &args.path, Some((args.line, args.character)), run).await?;
    let Prepared::Ready(ready) = prepared else {
        return Ok(prepared.unavailable());
    };
    let position = to_lsp(&ready.text, args.line, args.character).map_err(position_error)?;
    let points = match kind {
        LocateKind::Definition => {
            ready
                .client
                .definition(&ready.synced.uri, position, &run.cancel)
                .await
        }
        LocateKind::References => {
            ready
                .client
                .references(&ready.synced.uri, position, &run.cancel)
                .await
        }
    };
    match points {
        Ok(points) => {
            let (locations, truncated) = location_hits(&points, &ctx.root, &ready.filter, limit);
            Ok(json!({
                "available": true,
                "language": ready.language,
                "server": ready.server,
                "path": ready.display,
                "truncated": truncated,
                "locations": locations,
            }))
        }
        Err(LspError::Cancelled) => Err(ToolError::Cancelled),
        Err(err) => Ok(from_error(&err, Some(ready.language), Some(ready.server))),
    }
}

fn position_schema(path_help: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": path_help},
            "line": {"type": "integer", "minimum": 1, "description": "1-based line."},
            "character": {"type": "integer", "minimum": 1, "description": "1-based character on that line, counted in Unicode scalars."}
        },
        "required": ["path", "line", "character"],
        "additionalProperties": false
    })
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct PositionArgs {
    path: String,
    line: u32,
    character: u32,
}

#[derive(Deserialize)]
struct QueryArgs {
    query: String,
}

fn required_path(args: Value) -> Result<String, ToolError> {
    let args: PathArgs = serde_json::from_value(args)
        .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
    if args.path.is_empty() {
        return Err(ToolError::InvalidArgs("path is required".into()));
    }
    Ok(args.path)
}

fn position_args(args: Value) -> Result<PositionArgs, ToolError> {
    let args: PositionArgs = serde_json::from_value(args)
        .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
    if args.path.is_empty() {
        return Err(ToolError::InvalidArgs("path is required".into()));
    }
    if args.line == 0 {
        return Err(ToolError::InvalidArgs(
            "line is past the end of the file".into(),
        ));
    }
    if args.character == 0 {
        return Err(ToolError::InvalidArgs(
            "character is past the end of the line".into(),
        ));
    }
    Ok(args)
}

fn position_error(err: PositionError) -> ToolError {
    match err {
        PositionError::Line => ToolError::Failed("line is past the end of the file".into()),
        PositionError::Character => {
            ToolError::Failed("character is past the end of the line".into())
        }
    }
}

fn from_error(err: &LspError, language: Option<&str>, server: Option<&str>) -> Value {
    match err {
        LspError::Cancelled => {
            // The caller turns this into ToolError before returning. A leftover
            // path still tells the model the call stopped.
            unavailable("timeout", language, server, "cancelled")
        }
        LspError::Timeout => unavailable(
            "timeout",
            language,
            server,
            "The language server did not answer in time. Call again, or use the shell.",
        ),
        LspError::Stopped | LspError::Protocol(_) => unavailable(
            "server_failed",
            language,
            server,
            "The language server failed. Use the shell to compile, or grep.",
        ),
    }
}

fn reason_name(reason: &Unavailable) -> &'static str {
    match reason {
        Unavailable::NoServer => "no_server",
        Unavailable::ServerFailed(_) => "server_failed",
        Unavailable::Timeout => "timeout",
    }
}

fn hint(reason: &Unavailable, argv: &str) -> String {
    match reason {
        Unavailable::NoServer => {
            format!("{argv} is not on PATH. Use the shell to compile, or grep.")
        }
        Unavailable::ServerFailed(detail) => {
            if detail.is_empty() {
                format!("{argv} failed to start. Use the shell to compile, or grep.")
            } else {
                format!("{argv} failed to start: {detail} Use the shell to compile, or grep.")
            }
        }
        Unavailable::Timeout => {
            "The language server did not answer in time. Call again, or use the shell.".into()
        }
    }
}

fn unavailable(reason: &str, language: Option<&str>, server: Option<&str>, hint: &str) -> Value {
    let mut value = json!({
        "available": false,
        "reason": reason,
        "hint": hint,
    });
    if let Some(language) = language {
        value["language"] = json!(language);
    }
    if let Some(server) = server {
        value["server"] = json!(server);
    }
    value
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use robi_core::tool::{Tool, ToolRun};
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use crate::agent::lsp::connect_fake;
    use crate::agent::lsp::{LspHub, Timing};
    use crate::agent::tools::apply_tests::harness;
    use crate::agent::tools::context::ToolContext;

    use super::*;

    fn run() -> ToolRun {
        ToolRun::new(CancellationToken::new())
    }

    async fn ready() -> (super::super::apply_tests::Harness, Arc<ToolContext>) {
        let harness = harness().await;
        std::fs::write(harness.ctx.root.join("lib.rs"), "fn demo() {}\n").unwrap();
        let hub = LspHub::build(
            Timing::fast(),
            Arc::new(|_| Some(PathBuf::from("/bin/echo"))),
        );
        let fake = connect_fake(&harness.ctx.root, Timing::fast()).await;
        hub.install_for_test(&harness.ctx.root, "rust-analyzer", fake)
            .await;
        let ctx = Arc::new(ToolContext {
            session_id: harness.ctx.session_id,
            root: harness.ctx.root.clone(),
            sessions: Arc::clone(&harness.ctx.sessions),
            file_changes: Arc::clone(&harness.ctx.file_changes),
            index: None,
            lsp: hub,
            originals: None,
            lsp_enabled: true,
            settings: None,
        });
        (harness, ctx)
    }

    #[tokio::test]
    async fn diagnostics_and_navigation_use_the_server() {
        let (_harness, ctx) = ready().await;
        let path = "lib.rs";
        let diagnostics = Diagnostics::new(Arc::clone(&ctx))
            .execute(json!({ "path": path }), run())
            .await
            .unwrap();
        assert_eq!(diagnostics["available"], json!(true));
        assert_eq!(diagnostics["pending"], json!(false));
        assert_eq!(diagnostics["diagnostics"][0]["severity"], json!("error"));
        assert_eq!(diagnostics["diagnostics"][0]["code"], json!("E0425"));
        assert_eq!(diagnostics["diagnostics"][0]["line"], json!(1));

        let definition = Definition::new(Arc::clone(&ctx))
            .execute(json!({ "path": path, "line": 1, "character": 4 }), run())
            .await
            .unwrap();
        assert_eq!(definition["locations"][0]["path"], json!("lib.rs"));
        assert_eq!(definition["locations"][0]["character"], json!(4));
        assert_eq!(definition["locations"][0]["preview"], json!("fn demo() {}"));

        let hover = Hover::new(Arc::clone(&ctx))
            .execute(json!({ "path": path, "line": 1, "character": 4 }), run())
            .await
            .unwrap();
        assert_eq!(hover["contents"], json!("fn demo"));

        let symbols = WorkspaceSymbol::new(ctx)
            .execute(json!({ "query": "demo" }), run())
            .await
            .unwrap();
        assert_eq!(symbols["available"], json!(true));
        assert_eq!(symbols["symbols"][0]["name"], json!("demo"));
        assert_eq!(symbols["symbols"][0]["kind"], json!("function"));
        assert_eq!(symbols["symbols"][0]["line"], json!(1));
        assert_eq!(symbols["symbols"][0]["character"], json!(4));
    }

    #[tokio::test]
    async fn a_missing_server_does_not_fail_the_turn() {
        let harness = harness().await;
        std::fs::write(harness.ctx.root.join("lib.rs"), "fn demo() {}\n").unwrap();
        std::fs::write(harness.ctx.root.join("notes.md"), "hello\n").unwrap();
        let ctx = Arc::new(ToolContext {
            session_id: harness.ctx.session_id,
            root: harness.ctx.root.clone(),
            sessions: Arc::clone(&harness.ctx.sessions),
            file_changes: Arc::clone(&harness.ctx.file_changes),
            index: None,
            lsp: LspHub::build(Timing::fast(), Arc::new(|_| None)),
            originals: None,
            lsp_enabled: true,
            settings: None,
        });
        let missing = Diagnostics::new(Arc::clone(&ctx))
            .execute(json!({ "path": "lib.rs" }), run())
            .await
            .unwrap();
        assert_eq!(missing["available"], json!(false));
        assert_eq!(missing["reason"], json!("no_server"));
        assert_eq!(missing["language"], json!("rust"));

        let unsupported = Diagnostics::new(Arc::clone(&ctx))
            .execute(json!({ "path": "notes.md" }), run())
            .await
            .unwrap();
        assert_eq!(unsupported["reason"], json!("unsupported"));

        let outside = Diagnostics::new(Arc::clone(&ctx))
            .execute(json!({ "path": "/etc/hosts" }), run())
            .await
            .unwrap_err();
        assert_eq!(outside.to_string(), "path is outside the workspace");

        let past = Definition::new(Arc::clone(&ctx))
            .execute(
                json!({ "path": "lib.rs", "line": 9, "character": 1 }),
                run(),
            )
            .await
            .unwrap_err();
        assert_eq!(past.to_string(), "line is past the end of the file");

        let column = Definition::new(Arc::clone(&ctx))
            .execute(
                json!({ "path": "lib.rs", "line": 1, "character": 40 }),
                run(),
            )
            .await
            .unwrap_err();
        assert_eq!(column.to_string(), "character is past the end of the line");

        std::fs::write(harness.ctx.root.join(".env"), "TOKEN=1\n").unwrap();
        let denied = Diagnostics::new(Arc::clone(&ctx))
            .execute(json!({ "path": ".env" }), run())
            .await
            .unwrap_err();
        assert!(denied.to_string().starts_with("path is not allowed:"));

        let empty = WorkspaceSymbol::new(Arc::clone(&ctx))
            .execute(json!({ "query": "  " }), run())
            .await
            .unwrap_err();
        assert_eq!(empty.to_string(), "invalid arguments: query is empty");

        let big = vec![b'a'; (crate::agent::lsp::MAX_FILE_BYTES as usize) + 1];
        std::fs::write(harness.ctx.root.join("huge.rs"), big).unwrap();
        let large = Diagnostics::new(ctx)
            .execute(json!({ "path": "huge.rs" }), run())
            .await
            .unwrap_err();
        assert!(large.to_string().starts_with("file is too large:"));
    }
}
