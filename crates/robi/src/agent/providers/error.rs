//! Provider failures, and how they reach the loop.
//!
//! `robi_core::error::ModelError` has two variants and the loop needs no more, so
//! retryability is decided here rather than surfaced. This type keeps the HTTP
//! status, the provider's own message, and the reason for the classification
//! while the adapter owns them, and collapses to `ModelError` at the boundary.

use robi_core::error::ModelError;

/// Something that stopped a model turn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("no such model: '{0}'")]
    UnknownModel(String),

    #[error("model '{0}' cannot call tools, so it cannot drive the agent loop")]
    ToolLessModel(String),

    #[error("the provider rejected the credential")]
    Unauthorized { status: u16 },

    #[error("the provider is rate limiting requests")]
    RateLimited { status: u16 },

    #[error("the provider returned {status}: {body}")]
    Http { status: u16, body: String },

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

/// Trim a response body before it reaches an error message.
///
/// A provider can answer an error with a full HTML page. The first couple of
/// hundred bytes say what happened; the rest only makes the message unreadable.
pub fn truncate_body(body: &str) -> String {
    const LIMIT: usize = 512;
    if body.len() <= LIMIT {
        return body.trim().to_owned();
    }
    let mut cut = LIMIT;
    while cut > 0 && !body.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", body[..cut].trim())
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
        let error = ProviderError::RateLimited { status: 429 }.into_model_error();
        let ModelError::Provider(text) = error else {
            panic!("a rate limit is a provider error");
        };
        assert!(text.contains("rate limiting"), "{text}");
    }

    #[test]
    fn a_body_is_truncated_on_a_char_boundary() {
        let body = "é".repeat(1000);
        let short = truncate_body(&body);
        assert!(short.len() < body.len());
        assert!(short.ends_with('…'));
    }
}
