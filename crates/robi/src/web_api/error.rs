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
            ServiceError::Unknown => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "An internal error occurred".into(),
            ),
        };

        let body = Json(json!({ "error": error_message }));
        (status, body).into_response()
    }
}
