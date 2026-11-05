//! One error type for the HTTP layer, rendered as RFC 9457 `application/problem+json`.
//!
//! Clients get a stable `code` (UPPER_SNAKE_CASE, see context/wiki/architecture/api-mvp.md
//! "Errors"), a generic `title` and the `requestId`; internal details go to server logs only.

use axum::Json;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::request_id;

pub const PROBLEM_JSON: &str = "application/problem+json";

/// RFC 9457 problem details with two extension members: `code` and `requestId`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProblemDetails {
    /// URI reference identifying the problem type (`about:blank` = see `status` and `code`).
    #[serde(rename = "type")]
    #[schema(example = "about:blank")]
    pub problem_type: String,
    #[schema(example = "Not Found")]
    pub title: String,
    #[schema(example = 404)]
    pub status: u16,
    /// Safe, human-readable explanation. Never contains internal details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Stable error code clients can switch on.
    #[schema(example = "NOT_FOUND")]
    pub code: String,
    /// Id of the failing request (same value as the `X-Request-Id` response header).
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    #[schema(example = "0192f0c1-7c1e-7a3b-9d2e-123456789abc")]
    pub request_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The request is malformed or fails validation. `detail` must be safe to show.
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("unsupported media type")]
    UnsupportedMediaType,
    /// Resource missing, or the caller may not know it exists (object-level denials use this
    /// too, so "forbidden" and "missing" are indistinguishable).
    #[error("resource not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("request timed out")]
    RequestTimeout,
    #[error("internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl ApiError {
    pub fn internal(err: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::Internal(err.into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "VALIDATION_FAILED",
            Self::UnsupportedMediaType => "MEDIA_TYPE_UNSUPPORTED",
            Self::NotFound => "NOT_FOUND",
            Self::MethodNotAllowed => "METHOD_NOT_ALLOWED",
            Self::RequestTimeout => "REQUEST_TIMEOUT",
            Self::Internal(_) => "INTERNAL",
        }
    }

    fn detail(&self) -> Option<String> {
        match self {
            Self::Validation(detail) => Some(detail.clone()),
            _ => None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::Internal(source) = &self {
            // Logged inside the request span, so the request id is attached.
            tracing::error!(error = %source, "internal error");
        }
        problem_response(self.status(), self.code(), self.detail())
    }
}

/// Object-level denial helper: a missing row and a row the caller may not see both become
/// the same 404, so ids cannot be probed.
pub trait OrNotFound<T> {
    fn or_not_found(self) -> Result<T, ApiError>;
}

impl<T> OrNotFound<T> for Option<T> {
    fn or_not_found(self) -> Result<T, ApiError> {
        self.ok_or(ApiError::NotFound)
    }
}

/// Builds a problem+json response for the current request. Never put internal details in
/// `detail`.
pub fn problem_response(status: StatusCode, code: &str, detail: Option<String>) -> Response {
    let body = ProblemDetails {
        problem_type: "about:blank".to_owned(),
        title: status.canonical_reason().unwrap_or("Error").to_owned(),
        status: status.as_u16(),
        detail,
        code: code.to_owned(),
        request_id: request_id::current().map(|id| id.0.hyphenated().to_string()),
    };
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_upper_snake_case() {
        let errors = [
            ApiError::Validation("x".into()),
            ApiError::UnsupportedMediaType,
            ApiError::NotFound,
            ApiError::MethodNotAllowed,
            ApiError::RequestTimeout,
            ApiError::internal("boom"),
        ];
        for err in errors {
            let code = err.code();
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "{code} must be UPPER_SNAKE_CASE"
            );
        }
    }

    #[test]
    fn internal_errors_never_expose_details() {
        let response = ApiError::internal("password=hunter2").into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let err = ApiError::internal("password=hunter2");
        assert!(err.detail().is_none());
    }

    #[test]
    fn missing_option_becomes_not_found() {
        let missing: Option<u8> = None;
        assert!(matches!(missing.or_not_found(), Err(ApiError::NotFound)));
        assert_eq!(Some(1).or_not_found().ok(), Some(1));
    }
}
