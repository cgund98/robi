//! Tool-result compression. The shell pass is phases 1–3 of shell output.

mod mcp;
mod memory;
mod shell;
mod store;

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::compress::{CompressError, CompressOutcome, CompressRequest, Compressor};
use robi_core::ids::SessionId;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub use self::memory::MemoryOriginals;
use self::shell::compress_stream;
pub use self::shell::reduce_lines;
pub use self::store::{Inserted, Lookup, OriginalStore};

/// Collapses `shell` stdout and stderr, and line-oriented MCP text.
pub struct ShellCompressor {
    store: Arc<dyn OriginalStore>,
    session: SessionId,
}

impl ShellCompressor {
    pub fn new(store: Arc<dyn OriginalStore>, session: SessionId) -> Self {
        Self { store, session }
    }
}

#[async_trait]
impl Compressor for ShellCompressor {
    async fn compress(&self, request: CompressRequest) -> Result<CompressOutcome, CompressError> {
        if request.tool == "shell" {
            return compress_shell(self, request).await;
        }
        if request.tool.starts_with("mcp_") {
            if let Some(text) = request.result.as_str() {
                return compress_mcp(self, &request, text).await;
            }
        }
        Ok(CompressOutcome::unchanged(request.result))
    }
}

async fn compress_shell(
    compressor: &ShellCompressor,
    request: CompressRequest,
) -> Result<CompressOutcome, CompressError> {
    let stdout = request.result.get("stdout").and_then(Value::as_str);
    let stderr = request.result.get("stderr").and_then(Value::as_str);
    let (Some(stdout), Some(stderr)) = (stdout, stderr) else {
        return Ok(CompressOutcome::unchanged(request.result));
    };
    let next_stdout = compress_stream(stdout);
    let next_stderr = compress_stream(stderr);
    if next_stdout.is_none() && next_stderr.is_none() {
        return Ok(CompressOutcome::unchanged(request.result));
    }
    let exit_code = request
        .result
        .get("exit_code")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let truncated = request
        .result
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let body = json!({
        "stdout": stdout,
        "stderr": stderr,
        "exit_code": exit_code,
        "truncated": truncated,
    });
    let body = serde_json::to_string(&body).map_err(|err| CompressError(err.to_string()))?;
    let inserted = match compressor
        .store
        .insert(compressor.session, &request.tool_call_id, &body)
        .await
    {
        Ok(inserted) => inserted,
        Err(error) => {
            tracing::warn!(%error, "shell original was not stored");
            return Ok(CompressOutcome::unchanged(request.result));
        }
    };
    let mut result = request.result;
    if let Some(text) = next_stdout {
        result["stdout"] = json!(with_shell_header(&text, &inserted, exit_code));
    }
    if let Some(text) = next_stderr {
        result["stderr"] = json!(with_shell_header(&text, &inserted, exit_code));
    }
    Ok(CompressOutcome {
        result,
        original_id: Some(inserted.id),
        original_bytes: None,
    })
}

async fn compress_mcp(
    compressor: &ShellCompressor,
    request: &CompressRequest,
    text: &str,
) -> Result<CompressOutcome, CompressError> {
    let Some(collapsed) = mcp::compress_text(text) else {
        return Ok(CompressOutcome::unchanged(request.result.clone()));
    };
    let body = json!({
        "kind": "mcp",
        "text": text,
        "truncated": mcp::client_truncated(text),
    });
    let body = serde_json::to_string(&body).map_err(|err| CompressError(err.to_string()))?;
    let inserted = match compressor
        .store
        .insert(compressor.session, &request.tool_call_id, &body)
        .await
    {
        Ok(inserted) => inserted,
        Err(error) => {
            tracing::warn!(%error, "tool original was not stored");
            return Ok(CompressOutcome::unchanged(request.result.clone()));
        }
    };
    let text = format!(
        "{}\n{collapsed}",
        mcp::header(&inserted.id, &inserted.sha256)
    );
    Ok(CompressOutcome {
        result: json!(text),
        original_id: Some(inserted.id),
        original_bytes: None,
    })
}

fn with_shell_header(body: &str, inserted: &Inserted, exit_code: i64) -> String {
    format!(
        "{}\n{body}",
        shell::header(&inserted.id, &inserted.sha256, exit_code)
    )
}

pub fn sha256_hex(body: &str) -> String {
    let digest = Sha256::digest(body.as_bytes());
    format!("{digest:x}")
}

pub fn new_id() -> String {
    Uuid::now_v7().to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use robi_core::compress::{CompressRequest, Compressor};
    use robi_core::ids::SessionId;
    use serde_json::json;

    use super::memory::MemoryOriginals;
    use super::store::{Lookup, OriginalStore};
    use super::ShellCompressor;

    #[tokio::test]
    async fn a_large_shell_result_stores_the_capped_streams() {
        let store = Arc::new(MemoryOriginals::default());
        let session = SessionId::new();
        let compressor = ShellCompressor::new(store.clone(), session);
        let mut stdout = String::new();
        for index in 0..200 {
            stdout.push_str(&format!("test tests::test_auth_{index} ... ok\n"));
        }
        stdout.push_str("test result: ok. 200 passed\n");
        let request = CompressRequest {
            tool: "shell".into(),
            tool_call_id: "call".into(),
            session_id: session.to_string(),
            result: json!({
                "stdout": stdout,
                "stderr": "",
                "exit_code": 0,
                "truncated": false,
                "sandboxed": true,
            }),
        };
        let outcome = compressor.compress(request).await.unwrap();
        let id = outcome.original_id.expect("row");
        let shown = outcome.result["stdout"].as_str().unwrap();
        assert!(shown.contains(&id));
        assert!(shown.starts_with("<<<ROBI_LOG id="));
        assert!(outcome.result["sandboxed"].as_bool().unwrap());
        assert!(outcome.result["stderr"].as_str().unwrap().is_empty());
        match store.lookup(session, &id).await.unwrap() {
            Lookup::One(body) => assert!(body["stdout"].as_str().unwrap().contains("test_auth_0")),
            other => panic!("expected one row, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_store_failure_leaves_the_result_unmarked() {
        let compressor = ShellCompressor::new(Arc::new(Failing), SessionId::new());
        let mut stdout = String::new();
        for index in 0..200 {
            stdout.push_str(&format!("test tests::test_auth_{index} ... ok\n"));
        }
        let request = CompressRequest {
            tool: "shell".into(),
            tool_call_id: "call".into(),
            session_id: "session".into(),
            result: json!({"stdout": stdout, "stderr": "", "exit_code": 1, "truncated": false}),
        };
        let outcome = compressor.compress(request).await.unwrap();
        assert!(outcome.original_id.is_none());
        assert!(!outcome.result["stdout"]
            .as_str()
            .unwrap()
            .contains("<<<ROBI_LOG"));
    }

    #[tokio::test]
    async fn a_large_mcp_json_array_stores_kind_mcp() {
        let store = Arc::new(MemoryOriginals::default());
        let session = SessionId::new();
        let compressor = ShellCompressor::new(store.clone(), session);
        let mut items = Vec::new();
        for index in 0..200 {
            items.push(format!(
                r#"{{"id":"issue-{index}","title":"Pay invoice {index}","state":"open"}}"#
            ));
        }
        let text = format!("[{}]", items.join(","));
        let request = CompressRequest {
            tool: "mcp_linear_list_issues".into(),
            tool_call_id: "call".into(),
            session_id: session.to_string(),
            result: json!(text),
        };
        let outcome = compressor.compress(request).await.unwrap();
        let id = outcome.original_id.expect("row");
        let shown = outcome.result.as_str().unwrap();
        assert!(shown.starts_with("<<<ROBI_LOG id="));
        assert!(shown.contains("kind=mcp"));
        assert!(shown.contains(&id));
        match store.lookup(session, &id).await.unwrap() {
            Lookup::One(body) => {
                assert_eq!(body["kind"], "mcp");
                assert!(body["text"].as_str().unwrap().contains("issue-0"));
            }
            other => panic!("expected one row, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_short_mcp_result_is_unchanged() {
        let store = Arc::new(MemoryOriginals::default());
        let session = SessionId::new();
        let compressor = ShellCompressor::new(store.clone(), session);
        let request = CompressRequest {
            tool: "mcp_linear_get_issue".into(),
            tool_call_id: "call".into(),
            session_id: session.to_string(),
            result: json!("{\"id\":\"one\"}"),
        };
        let outcome = compressor.compress(request).await.unwrap();
        assert!(outcome.original_id.is_none());
        assert_eq!(outcome.result, json!("{\"id\":\"one\"}"));
        assert!(matches!(
            store.lookup(session, "missing").await.unwrap(),
            Lookup::Missing
        ));
    }

    #[tokio::test]
    async fn an_mcp_store_failure_returns_the_input() {
        let compressor = ShellCompressor::new(Arc::new(Failing), SessionId::new());
        let mut items = Vec::new();
        for index in 0..200 {
            items.push(format!(
                r#"{{"id":"issue-{index}","title":"Pay invoice {index}"}}"#
            ));
        }
        let text = format!("[{}]", items.join(","));
        let request = CompressRequest {
            tool: "mcp_linear_list_issues".into(),
            tool_call_id: "call".into(),
            session_id: "session".into(),
            result: json!(text.clone()),
        };
        let outcome = compressor.compress(request).await.unwrap();
        assert!(outcome.original_id.is_none());
        assert_eq!(outcome.result, json!(text));
    }

    #[tokio::test]
    async fn a_non_mcp_tool_is_unchanged_by_the_mcp_branch() {
        let compressor =
            ShellCompressor::new(Arc::new(MemoryOriginals::default()), SessionId::new());
        let text = format!("[{}]", r#"{"id":"x","title":"y"}"#.repeat(200));
        let request = CompressRequest {
            tool: "read_file".into(),
            tool_call_id: "call".into(),
            session_id: "session".into(),
            result: json!(text.clone()),
        };
        let outcome = compressor.compress(request).await.unwrap();
        assert!(outcome.original_id.is_none());
        assert_eq!(outcome.result, json!(text));
    }

    struct Failing;

    #[async_trait::async_trait]
    impl super::OriginalStore for Failing {
        async fn insert(
            &self,
            _session: SessionId,
            _tool_call_id: &str,
            _body: &str,
        ) -> Result<super::Inserted, String> {
            Err("disk full".into())
        }

        async fn lookup(&self, _session: SessionId, _id: &str) -> Result<super::Lookup, String> {
            Err("disk full".into())
        }
    }
}
