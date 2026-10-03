//! Text the model reads from a `tools/call` result.

use serde_json::Value;

use super::names::MAX_RESULT_BYTES;

/// Join text blocks. `Err` is the tool-error message.
pub fn render_owned(result: &Value) -> Result<String, String> {
    let text = blocks(result);
    if text.is_empty() {
        return Err("empty tool result".into());
    }
    let text = bound(&text);
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        Err(text)
    } else {
        Ok(text)
    }
}

fn blocks(result: &Value) -> String {
    let Some(content) = result.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    let mut lines = Vec::new();
    for block in content {
        let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    lines.push(text.to_owned());
                }
            }
            "image" | "audio" => lines.push(format!("[{kind} omitted]")),
            "resource" => {
                let uri = block
                    .get("resource")
                    .and_then(|resource| resource.get("uri"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if uri.is_empty() {
                    lines.push("[resource omitted]".into());
                } else {
                    lines.push(uri.to_owned());
                }
            }
            "resource_link" => {
                let uri = block.get("uri").and_then(Value::as_str).unwrap_or("");
                if !uri.is_empty() {
                    lines.push(uri.to_owned());
                }
            }
            other if !other.is_empty() => lines.push(format!("[{other} omitted]")),
            _ => {}
        }
    }
    lines.join("\n")
}

fn bound(text: &str) -> String {
    if text.len() <= MAX_RESULT_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_RESULT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[truncated at {} bytes]",
        &text[..end],
        MAX_RESULT_BYTES
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_is_error_result_is_a_tool_error() {
        let result = json!({"isError": true, "content": [{"type": "text", "text": "nope"}]});
        assert_eq!(render_owned(&result).unwrap_err(), "nope");
    }

    #[test]
    fn an_image_is_a_placeholder() {
        let result = json!({"content": [{"type": "image", "data": "aaaa"}]});
        assert_eq!(render_owned(&result).unwrap(), "[image omitted]");
    }
}
