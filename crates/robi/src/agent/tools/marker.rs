//! Refuse outline and shell markers before a write starts.

use robi_core::error::ToolError;

const PREFIX: &str = "<<<ROBI_";

pub(crate) fn reject_marker(text: &str) -> Result<(), ToolError> {
    let Some(at) = text.find(PREFIX) else {
        return Ok(());
    };
    let marker = &text[at..];
    if marker.starts_with("<<<ROBI_LOG") {
        return Err(ToolError::Failed(
            "text contains a compressed shell marker. It is not file text.".into(),
        ));
    }
    let mut message = "text contains an omitted-body marker. It is not in the file. Call read_code with focus_symbols set to the symbol in that marker, or read_file at that marker's start line, and quote those bytes.".to_owned();
    if let Some((symbol, lines)) = omitted_symbol(marker) {
        message.push_str(&format!(" symbol: {symbol}, lines: {lines}."));
    }
    Err(ToolError::Failed(message))
}

fn omitted_symbol(marker: &str) -> Option<(String, String)> {
    if !marker.starts_with("<<<ROBI_OMITTED") {
        return None;
    }
    let symbol = attr(marker, "symbol=")?;
    let lines = attr(marker, "lines=")?;
    Some((symbol, lines))
}

fn attr(marker: &str, key: &str) -> Option<String> {
    let rest = marker.split_once(key)?.1;
    if let Some(quoted) = rest.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = quoted.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                out.push(chars.next()?);
                continue;
            }
            if ch == '"' {
                return Some(out);
            }
            out.push(ch);
        }
        return None;
    }
    let end = rest.find(|ch: char| ch.is_whitespace() || ch == '>')?;
    Some(rest[..end].to_owned())
}
