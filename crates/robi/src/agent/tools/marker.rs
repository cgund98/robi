//! Refuse a compressed shell marker before a write starts.

use robi_core::error::ToolError;

const PREFIX: &str = "<<<ROBI_LOG";

pub(crate) fn reject_marker(text: &str) -> Result<(), ToolError> {
    if text.contains(PREFIX) {
        return Err(ToolError::Failed(
            "text contains a compressed shell marker. It is not file text.".into(),
        ));
    }
    Ok(())
}
