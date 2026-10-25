use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::domain::error::ServiceError;

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            ServiceError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ServiceError::NotFound(id) => (StatusCode::NOT_FOUND, format!("Entity {id} not found")),
            ServiceError::Conflict(message) => (StatusCode::CONFLICT, message),
            ServiceError::PayloadTooLarge(message) => (StatusCode::PAYLOAD_TOO_LARGE, message),
            ServiceError::UnsupportedMediaType(message) => {
                (StatusCode::UNSUPPORTED_MEDIA_TYPE, message)
            }
            ServiceError::Unknown => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "An internal error occurred".into(),
            ),
        };

        let body = Json(json!({ "error": error_message }));
        (status, body).into_response()
    }
}

/// The two refusals the file opener can return are the reader's mistake, not the
/// server's: a path outside the workspace, and a template that cannot render.
/// The status is the same `400` the routes returned before the opener moved.
impl From<crate::agent::workspace::OpenError> for ServiceError {
    fn from(error: crate::agent::workspace::OpenError) -> Self {
        ServiceError::BadRequest(error.to_string())
    }
}

/// A path that is not a file inside the workspace. `400`, as it always was.
impl From<crate::agent::workspace::PathError> for ServiceError {
    fn from(error: crate::agent::workspace::PathError) -> Self {
        ServiceError::BadRequest(error.to_string())
    }
}
