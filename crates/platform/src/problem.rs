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
    /// No credentials, or credentials that do not identify a live session.
    #[error("unauthenticated")]
    Unauthenticated,
    /// The access token expired: refresh and retry.
    #[error("session expired")]
    SessionExpired,
    /// The session was revoked (logout, refresh-token reuse, suspension): sign in again.
    #[error("session revoked")]
    SessionRevoked,
    /// Authenticated, but this kind of session may not call the route.
    #[error("forbidden")]
    Forbidden,
    /// Neutral age-gate refusal (no threshold or region is revealed).
    #[error("age gate refused")]
    AgeGateRefused,
    /// The date of birth was already submitted; it cannot be changed (DOB locked).
    #[error("age already submitted")]
    AgeAlreadySubmitted,
    /// A full session is required; the caller is still onboarding.
    #[error("onboarding incomplete")]
    OnboardingIncomplete,
    #[error("account suspended")]
    AccountSuspended,
    /// The device attestation of a request marked `att` failed (enforce mode only).
    #[error("attestation failed")]
    AttestationFailed,
    /// The handle belongs to, or is reserved for, someone else (no detail which).
    #[error("handle taken")]
    HandleTaken,
    /// The handle was changed too recently.
    #[error("handle change cooldown")]
    HandleChangeCooldown { retry_after_secs: u64 },
    /// The Idempotency-Key was already used with a different request.
    #[error("idempotency key reused")]
    IdempotencyKeyReused,
    /// The first request with this Idempotency-Key is still running: retry later.
    #[error("idempotency key in progress")]
    IdempotencyInProgress,
    /// Resource missing, or the caller may not know it exists (object-level denials use this
    /// too, so "forbidden" and "missing" are indistinguishable).
    #[error("resource not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("request timed out")]
    RequestTimeout,
    /// `X-App-Version` is below the minimum supported version.
    #[error("upgrade required")]
    UpgradeRequired,
    /// The caller exhausted a rate limit.
    #[error("rate limited")]
    RateLimited { retry_after_secs: u64 },
    /// A dependency is down and the route fails closed.
    #[error("temporarily unavailable")]
    Unavailable { retry_after_secs: u64 },
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
            Self::Unauthenticated | Self::SessionExpired | Self::SessionRevoked => {
                StatusCode::UNAUTHORIZED
            }
            Self::Forbidden
            | Self::OnboardingIncomplete
            | Self::AccountSuspended
            | Self::AttestationFailed
            | Self::AgeGateRefused
            | Self::AgeAlreadySubmitted => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::HandleTaken | Self::IdempotencyInProgress => StatusCode::CONFLICT,
            Self::IdempotencyKeyReused => StatusCode::UNPROCESSABLE_ENTITY,
            Self::HandleChangeCooldown { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            Self::UpgradeRequired => StatusCode::UPGRADE_REQUIRED,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "VALIDATION_FAILED",
            Self::UnsupportedMediaType => "MEDIA_TYPE_UNSUPPORTED",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::SessionExpired => "SESSION_EXPIRED",
            Self::SessionRevoked => "SESSION_REVOKED",
            Self::Forbidden => "FORBIDDEN",
            Self::OnboardingIncomplete => "ONBOARDING_INCOMPLETE",
            Self::AccountSuspended => "ACCOUNT_SUSPENDED",
            Self::AttestationFailed => "ATTESTATION_FAILED",
            Self::AgeGateRefused => "AGE_GATE_REFUSED",
            Self::AgeAlreadySubmitted => "AGE_ALREADY_SUBMITTED",
            Self::NotFound => "NOT_FOUND",
            Self::HandleTaken => "HANDLE_TAKEN",
            Self::IdempotencyKeyReused => "IDEMPOTENCY_KEY_REUSED",
            Self::IdempotencyInProgress => "IDEMPOTENCY_IN_PROGRESS",
            Self::HandleChangeCooldown { .. } => "HANDLE_CHANGE_COOLDOWN",
            Self::MethodNotAllowed => "METHOD_NOT_ALLOWED",
            Self::RequestTimeout => "REQUEST_TIMEOUT",
            Self::UpgradeRequired => "UPGRADE_REQUIRED",
            Self::RateLimited { .. } => "RATE_LIMITED",
            Self::Unavailable { .. } => "TEMPORARILY_UNAVAILABLE",
            Self::Internal(_) => "INTERNAL",
        }
    }

    fn retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_after_secs }
            | Self::Unavailable { retry_after_secs }
            | Self::HandleChangeCooldown { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
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
        let mut response = problem_response(self.status(), self.code(), self.detail());
        if let Some(secs) = self.retry_after_secs() {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(secs));
        }
        response
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
            ApiError::Unauthenticated,
            ApiError::SessionExpired,
            ApiError::SessionRevoked,
            ApiError::Forbidden,
            ApiError::OnboardingIncomplete,
            ApiError::AccountSuspended,
            ApiError::AttestationFailed,
            ApiError::AgeGateRefused,
            ApiError::AgeAlreadySubmitted,
            ApiError::Unavailable {
                retry_after_secs: 1,
            },
            ApiError::RateLimited {
                retry_after_secs: 1,
            },
            ApiError::UpgradeRequired,
            ApiError::NotFound,
            ApiError::IdempotencyKeyReused,
            ApiError::IdempotencyInProgress,
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
    fn unavailable_carries_retry_after() {
        let response = ApiError::Unavailable {
            retry_after_secs: 7,
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(header::RETRY_AFTER),
            Some(&HeaderValue::from(7u64))
        );
    }

    #[test]
    fn missing_option_becomes_not_found() {
        let missing: Option<u8> = None;
        assert!(matches!(missing.or_not_found(), Err(ApiError::NotFound)));
        assert_eq!(Some(1).or_not_found().ok(), Some(1));
    }
}
