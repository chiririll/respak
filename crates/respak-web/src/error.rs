//! Errors as the API returns them: `{"error": {"kind": "...", "message": "..."}}`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// An error with the status it is returned with.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    kind: String,
    message: String,
}

/// Result of a handler.
pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    /// An error of any status.
    pub fn new(status: StatusCode, kind: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            kind: kind.to_owned(),
            message: message.into(),
        }
    }

    /// 400.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "BadRequest", message)
    }

    /// 404.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "NotFound", message)
    }

    /// 409: there is no session to answer with.
    pub fn not_connected() -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "NotConnected",
            "not connected: choose a source in the settings and connect",
        )
    }
}

impl From<respak_core::Error> for ApiError {
    fn from(error: respak_core::Error) -> Self {
        use respak_core::ErrorKind as K;
        let status = match error.kind() {
            K::NotFound { .. } | K::NoMatch { .. } => StatusCode::NOT_FOUND,
            K::InvalidRequest { .. } | K::InvalidOptions { .. } | K::NoPath { .. } => {
                StatusCode::BAD_REQUEST
            }
            K::Cancelled => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.kind().name(), error.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "Error", format!("{error:#}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": { "kind": self.kind, "message": self.message } });
        (self.status, Json(body)).into_response()
    }
}
