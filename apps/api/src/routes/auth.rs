//! Session endpoints (T-18, ADR-0004): refresh-token rotation and logout.

use axum::Json;
use axum::extract::State;
use http::StatusCode;
use identity::{IssuedSession, RefreshError, RevokeReason};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;

use crate::access::{Caller, ValidJson};
use crate::state::AppState;

pub const TAG: &str = "auth";

/// Body of `POST /v1/auth/refresh`. Only the refresh token: the session (and therefore the
/// account) is derived from it, never from a client-supplied id.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    #[schema(min_length = 1, max_length = 256)]
    pub refresh_token: String,
}

/// A new access + refresh token pair. Store both in the device's secure storage only.
#[derive(Debug, Serialize, ToSchema)]
pub struct TokenPair {
    #[schema(example = "Bearer")]
    pub token_type: &'static str,
    /// Opaque, 10 minutes. Send as `Authorization: Bearer <access_token>`.
    pub access_token: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub access_expires_at: OffsetDateTime,
    /// Opaque, single use. Replaying a used one signs out the whole session.
    pub refresh_token: String,
}

impl From<IssuedSession> for TokenPair {
    fn from(session: IssuedSession) -> Self {
        Self {
            token_type: "Bearer",
            access_token: session.access_token.expose().to_owned(),
            access_expires_at: session.access_expires_at,
            refresh_token: session.refresh_token.expose().to_owned(),
        }
    }
}

/// Rotate a refresh token. Auth: none (the refresh token in the body is the credential).
#[utoipa::path(
    post,
    path = "/v1/auth/refresh",
    tag = TAG,
    operation_id = "refreshSession",
    security(()),
    request_body = RefreshRequest,
    responses(
        (status = OK, description = "New token pair; the presented refresh token is now used up.", body = TokenPair),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (unknown fields are rejected).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED (unknown token) or SESSION_REVOKED (reused, revoked or ended session: sign in again).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE (rate-limit store down; auth fails closed).", body = ProblemDetails),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    body: ValidJson<RefreshRequest>,
) -> Result<Json<TokenPair>, ApiError> {
    let token = body.0.refresh_token;
    if token.is_empty() || token.len() > 256 {
        return Err(ApiError::Validation(
            "refresh_token must be 1 to 256 characters".to_owned(),
        ));
    }
    match state.identity.refresh(&token).await {
        Ok(session) => Ok(Json(session.into())),
        Err(RefreshError::Unknown) => Err(ApiError::Unauthenticated),
        Err(RefreshError::Revoked | RefreshError::Reused) => Err(ApiError::SessionRevoked),
        Err(RefreshError::Store(err)) => Err(ApiError::internal(err)),
    }
}

/// Log out this device: revokes the current session (and its widget token). Auth: onb/ses.
#[utoipa::path(
    delete,
    path = "/v1/sessions/current",
    tag = TAG,
    operation_id = "deleteCurrentSession",
    security(("bearer" = [])),
    responses(
        (status = NO_CONTENT, description = "Session revoked; its tokens stop working immediately."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
    )
)]
pub async fn delete_current_session(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<StatusCode, ApiError> {
    state
        .identity
        .revoke_session(caller.0.session_id, RevokeReason::Logout)
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}
