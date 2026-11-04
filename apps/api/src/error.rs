//! One error type for the HTTP layer, rendered as RFC 9457 `application/problem+json`.
//! Clients get a generic message; details go to server logs only.

use axum::Json;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use serde::Serialize;
use utoipa::ToSchema;

pub const PROBLEM_JSON: &str = "application/problem+json";

/// RFC 9457 problem details. `code` is a stable machine-readable extension member.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemDetails {
    /// URI reference identifying the problem type (`about:blank` = see `status`).
    #[serde(rename = "type")]
    #[schema(example = "about:blank")]
    pub problem_type: String,
    #[schema(example = "Not Found")]
    pub title: String,
    #[schema(example = 404)]
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Stable error code clients can switch on.
    #[schema(example = "not_found")]
    pub code: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
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

    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::RequestTimeout => "request_timeout",
            Self::Internal(_) => "internal",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::Internal(source) = &self {
            // Logged inside the request span, so the request id is attached.
            tracing::error!(error = %source, "internal error");
        }
        let status = self.status();
        problem_response(status, self.code(), None)
    }
}

/// Builds a problem+json response. Never put internal details in `detail`.
pub fn problem_response(status: StatusCode, code: &str, detail: Option<String>) -> Response {
    let body = ProblemDetails {
        problem_type: "about:blank".to_owned(),
        title: status.canonical_reason().unwrap_or("Error").to_owned(),
        status: status.as_u16(),
        detail,
        code: code.to_owned(),
    };
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON));
    response
}
