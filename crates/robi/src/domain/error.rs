use thiserror::Error;

/// A failure a service can return.
///
/// The web layer maps each variant to a status. Callers outside HTTP use the
/// same enum, so a long title is `BadRequest` whether or not a socket is open.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Entity not found for id: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Payload too large: {0}")]
    PayloadTooLarge(String),

    #[error("Unsupported media type: {0}")]
    UnsupportedMediaType(String),

    #[error("Internal server error")]
    Unknown,
}

impl ServiceError {
    /// The sentence to put in front of a person.
    ///
    /// `Display` carries the kind, which is right in a log and wrong in the
    /// terminal footer: `Conflict: that chat is open in another process (pid
    /// 4812)` reads as a stack trace. A conflict's own message is already a
    /// sentence, so it is returned as written.
    pub fn user_message(&self) -> String {
        match self {
            Self::Conflict(message) => message.clone(),
            other => other.to_string(),
        }
    }
}

impl From<crate::agent::providers::ProviderError> for ServiceError {
    fn from(error: crate::agent::providers::ProviderError) -> Self {
        // The HTTP body stays the fixed Unknown sentence. The detail has to be
        // logged here, because this is the last place that still has it.
        tracing::error!(error = %error, "provider error returned as an internal error");
        ServiceError::Unknown
    }
}
