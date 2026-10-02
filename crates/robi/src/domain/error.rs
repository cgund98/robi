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

    #[error("Internal server error")]
    Unknown,
}
