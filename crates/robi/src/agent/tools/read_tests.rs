//! Tests for the read tools that only exist behind `read-tools`.

use std::sync::Arc;

use robi_core::tool::{Tool, ToolRun};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::read_code::ReadCode;
use super::test_harness::harness;

#[tokio::test]
async fn read_code_outlines_a_rust_file() {
    let harness = harness().await;
    std::fs::write(
        harness.root.join("pay.rs"),
        "fn one() {\n    let value = 1;\n}\nfn two() {\n    let value = 2;\n}\n",
    )
    .unwrap();
    let tool = ReadCode::new(Arc::clone(&harness.ctx));
    let result = tool
        .execute(
            json!({"path": "pay.rs", "focus_symbols": ["one"]}),
            ToolRun::new(CancellationToken::new()),
        )
        .await
        .unwrap();
    assert_eq!(result["view"], "outline");
    let content = result["content"].as_str().unwrap();
    assert!(content.contains("let value = 1;"), "{content}");
    // A folded body is announced by the omitted marker; only the stem matters.
    let folded = format!("{}{}", "ROBI_", "OMITTED");
    assert!(content.contains(&folded), "{content}");
    assert!(!content.contains("let value = 2;"), "{content}");
}
