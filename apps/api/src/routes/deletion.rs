//! Account deletion (T-151, US-29 AC-29.1..29.3): request (fresh re-auth), show and cancel.
//!
//! Requesting logs the account out on every device and hides its content at once (every module's
//! `hide`): the account's shared boards are deleted, it leaves other boards with its pins, its
//! public links are revoked. Signing in again within the grace period gives a session that can
//! only read `/v1/me`, show and cancel the deletion, and log out. Erasure follows the grace
//! period (T-152).

use axum::Json;
use axum::extract::State;
use http::StatusCode;
use platform::problem::{ApiError, ProblemDetails};
use privacy::{DeletionRequest, RequestOutcome};
use serde::Serialize;
use time::OffsetDateTime;
use utoipa::ToSchema;

use crate::access::Caller;
use crate::routes::me::TAG;
use crate::state::AppState;

/// A pending deletion.
#[derive(Debug, Serialize, ToSchema)]
pub struct DeletionView {
    /// Always `pending`.
    #[schema(example = "pending")]
    pub status: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub requested_at: OffsetDateTime,
    /// Until then signing in again and `DELETE /v1/me/deletion` cancel it; afterwards it is final.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub grace_until: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeletionEnvelope {
    pub deletion: DeletionView,
}

impl From<DeletionRequest> for DeletionEnvelope {
    fn from(request: DeletionRequest) -> Self {
        Self {
            deletion: DeletionView {
                status: "pending",
                requested_at: request.requested_at,
                grace_until: request.grace_until,
            },
        }
    }
}

/// Request deletion of the caller's account. Auth: re (session + fresh `X-Reauth`).
///
/// Logs out every device (widget and re-auth tokens included), hides the account's content from
/// friends, boards and public links at once, deletes the shared boards it owns and removes its
/// pins from other boards (AC-29.1, AC-29.3; these stay done if the deletion is cancelled).
/// Requesting again while pending returns the same request.
#[utoipa::path(
    post,
    path = "/v1/me/deletion",
    tag = TAG,
    operation_id = "requestAccountDeletion",
    security(("bearer" = [])),
    params(("X-Reauth" = String, Header, description = "Re-auth token from POST /v1/auth/reauth (5 minutes).")),
    responses(
        (status = ACCEPTED, description = "Deletion pending; this session is logged out too.", body = DeletionEnvelope),
        (status = UNAUTHORIZED, description = "REAUTH_REQUIRED (missing, wrong or older than 5 minutes), UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn request_deletion(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<(StatusCode, Json<DeletionEnvelope>), ApiError> {
    match state
        .deletion()
        .request(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?
    {
        RequestOutcome::Requested(request) => Ok((StatusCode::ACCEPTED, Json(request.into()))),
        RequestOutcome::NotActive => Err(ApiError::Forbidden),
    }
}

/// The caller's pending deletion. Auth: ses (also a deletion-pending account's session).
#[utoipa::path(
    get,
    path = "/v1/me/deletion",
    tag = TAG,
    operation_id = "getAccountDeletion",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The pending deletion.", body = DeletionEnvelope),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no pending deletion.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn get_deletion(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<DeletionEnvelope>, ApiError> {
    let pending = state
        .deletion()
        .pending(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(pending.into()))
}

/// Cancel the caller's pending deletion within the grace period. Auth: ses (also a
/// deletion-pending account's session).
#[utoipa::path(
    delete,
    path = "/v1/me/deletion",
    tag = TAG,
    operation_id = "cancelAccountDeletion",
    security(("bearer" = [])),
    responses(
        (status = NO_CONTENT, description = "Cancelled: the account is active again."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no pending deletion, or the grace period is over.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn cancel_deletion(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<StatusCode, ApiError> {
    if state
        .deletion()
        .cancel(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
