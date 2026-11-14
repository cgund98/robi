//! Provider failures, and how they reach the loop.
//!
//! `robi_core::error::ModelError` has two variants and the loop needs no more, so
//! retryability is decided here rather than surfaced. This type keeps the HTTP
//! status, the provider's own message, and the reason for the classification
//! while the adapter owns them, and collapses to `ModelError` at the boundary.

use reqwest::StatusCode;
use robi_core::error::ModelError;

/// Ceiling on a raw body that is not JSON. A gateway can answer a failure with a
/// whole HTML page; the visible text of that page is the useful part, and a few
/// thousand bytes of it is more than a turn needs.
const RAW_BODY_LIMIT: usize = 4000;

/// Something that stopped a model turn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("no such model: '{0}'")]
    UnknownModel(String),

    #[error("model '{0}' cannot call tools, so it cannot drive the agent loop")]
    ToolLessModel(String),

    #[error("the provider rejected the credential ({status}): {detail}")]
    Unauthorized { status: u16, detail: String },

    #[error("the provider is rate limiting requests ({status}): {detail}")]
    RateLimited { status: u16, detail: String },

    #[error("the provider returned {status}: {detail}")]
    Http { status: u16, detail: String },

    #[error("could not reach the provider: {0}")]
    Transport(String),

    #[error("the provider sent a response this client could not read: {0}")]
    Malformed(String),

    #[error("the provider reported an error mid-stream: {0}")]
    Reported(String),

    #[error("the provider closed the stream before finishing the message")]
    StreamClosed,

    #[error("image {id} is missing from the store")]
    MissingImage { id: String },

    #[error("model '{model}' does not accept image input")]
    NoVision { model: String },

    #[error("the request was cancelled")]
    Cancelled,
}

impl ProviderError {
    /// The failure the loop reads.
    ///
    /// The status and a stable kind word go into the text, because the two-variant
    /// `ModelError` has nowhere else to put them. A structured variant is deferred
    /// until M2's UI has to render a rate limit differently from a bad key.
    pub fn into_model_error(self) -> ModelError {
        match self {
            ProviderError::StreamClosed => ModelError::StreamClosed,
            other => ModelError::Provider(other.to_string()),
        }
    }
}

/// The text a failed response carries, kept whole.
///
/// An error body is the only place a provider explains itself: which quota ran
/// out, which key is wrong, which parameter it rejected. So the JSON message is
/// preferred over the envelope around it, with the provider's own `type` and
/// `code` beside it — `insufficient_quota` separates an empty balance from a bad
/// key. A body that is not JSON is read as it arrived, with an HTML page reduced
/// to its visible text. Nothing is cut before the reason: a 512-byte window ends
/// in the middle of the sentence that says what happened.
pub fn error_detail(status: StatusCode, body: &str) -> String {
    match json_detail(body) {
        Some(detail) => detail,
        None => plain_body(body).unwrap_or_else(|| status_reason(status)),
    }
}

/// `message` from a JSON error body, with `type` and `code` when the provider
/// sent them. OpenAI, Anthropic, and the OpenAI-compatible servers all nest it
/// under `error`; a few put it at the top level.
fn json_detail(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let error = value.get("error").unwrap_or(&value);
    // `{"error": "bad key"}` is a body with no envelope around it.
    if let Some(text) = error.as_str() {
        return Some(text.to_owned());
    }
    let message = error
        .get("message")
        .and_then(|value| value.as_str())
        .or_else(|| value.get("message").and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|text| !text.is_empty())?;

    let mut detail = message.to_owned();
    let mut trailing: Vec<String> = Vec::new();
    if let Some(kind) = error.get("type").and_then(|value| value.as_str()) {
        trailing.push(kind.to_owned());
    }
    if let Some(code) = error.get("code").and_then(|value| value.as_str()) {
        if !trailing.iter().any(|item| item == code) {
            trailing.push(code.to_owned());
        }
    }
    if !trailing.is_empty() {
        detail.push_str(&format!(" ({})", trailing.join(", ")));
    }
    Some(detail)
}

/// A body that is not JSON. An HTML page is reduced to its text, because the
/// markup only buries the sentence underneath.
fn plain_body(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let text = if looks_like_html(trimmed) {
        collapse(&strip_tags(trimmed))
    } else {
        collapse(trimmed)
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(clip(text, RAW_BODY_LIMIT))
}

fn looks_like_html(body: &str) -> bool {
    let head: String = body.chars().take(200).collect::<String>().to_lowercase();
    head.contains("<!doctype html") || head.contains("<html") || head.contains("<body")
}

/// Drop tags, and the content of `script` and `style`, which is never prose.
fn strip_tags(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start + 1..start + end];
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        if name == "script" || name == "style" {
            let close = format!("</{name}");
            match rest[start..].to_lowercase().find(&close) {
                Some(offset) => {
                    rest = &rest[start + offset..];
                    let skip = rest.find('>').map(|at| at + 1).unwrap_or(rest.len());
                    rest = &rest[skip..];
                    continue;
                }
                None => break,
            }
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

/// One line, no runs of whitespace. A banner is one paragraph.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn status_reason(status: StatusCode) -> String {
    match status.canonical_reason() {
        Some(reason) => reason.to_lowercase(),
        None => status.as_u16().to_string(),
    }
}

/// Cut on a char boundary, and mark the cut so it is not read as the end.
fn clip(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut cut = limit;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", text[..cut].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_stream_keeps_its_own_variant() {
        // The loop treats `StreamClosed` differently from a provider message, so
        // the mapping must not flatten it.
        assert_eq!(
            ProviderError::StreamClosed.into_model_error(),
            ModelError::StreamClosed
        );
    }

    #[test]
    fn a_status_reaches_the_model_error_text() {
        let error = ProviderError::RateLimited {
            status: 429,
            detail: "slow down".to_owned(),
        }
        .into_model_error();
        let ModelError::Provider(text) = error else {
            panic!("a rate limit is a provider error");
        };
        assert!(text.contains("rate limiting"), "{text}");
        assert!(text.contains("429"), "{text}");
        assert!(text.contains("slow down"), "{text}");
    }

    #[test]
    fn the_providers_own_message_is_preferred_over_the_envelope() {
        let body = r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota","code":"insufficient_quota"}}"#;
        assert_eq!(
            error_detail(StatusCode::TOO_MANY_REQUESTS, body),
            "You exceeded your current quota (insufficient_quota)"
        );
    }

    #[test]
    fn a_bare_error_string_is_read_as_the_message() {
        assert_eq!(
            error_detail(StatusCode::UNAUTHORIZED, r#"{"error":"bad key"}"#),
            "bad key"
        );
    }

    #[test]
    fn the_reason_is_kept_whole() {
        // The whole sentence is the point: a cut before it says nothing.
        let message = "x".repeat(2000);
        let body = format!(r#"{{"error":{{"message":"{message}"}}}}"#);
        assert_eq!(error_detail(StatusCode::BAD_REQUEST, &body).len(), 2000);
    }

    #[test]
    fn an_html_page_is_reduced_to_its_text() {
        let body = "<!DOCTYPE html><html><head><style>p{color:red}</style></head><body><h1>502 Bad Gateway</h1><p>The upstream model timed out.</p></body></html>";
        let detail = error_detail(StatusCode::BAD_GATEWAY, body);
        assert!(detail.contains("502 Bad Gateway"), "{detail}");
        assert!(detail.contains("The upstream model timed out."), "{detail}");
        assert!(!detail.contains('<'), "{detail}");
        assert!(!detail.contains("color:red"), "{detail}");
    }

    #[test]
    fn an_empty_body_falls_back_to_the_status_reason() {
        assert_eq!(
            error_detail(StatusCode::INTERNAL_SERVER_ERROR, "  "),
            "internal server error"
        );
    }

    #[test]
    fn a_zero_body_name_is_not_reported_as_a_shortening() {
        // A plain-text body is read as it arrived, one line.
        assert_eq!(
            error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error\n\n  upstream died  "
            ),
            "Internal Server Error upstream died"
        );
    }
}
