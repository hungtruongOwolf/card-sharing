//! Personal invite links (T-37, US-4, SR-20). Both routes act on the caller's own invite only:
//! no path or body parameter can name another account.

use axum::Json;
use axum::extract::State;
use platform::problem::{ApiError, ProblemDetails};
use serde::Serialize;
use social::MyInvite;
use utoipa::ToSchema;

use crate::access::Caller;
use crate::state::AppState;

pub const TAG: &str = "social";

/// My personal invite link. Share it as a link or QR code.
#[derive(Debug, Serialize, ToSchema)]
pub struct InviteResponse {
    /// `<share host>/i/<token>`; the token is 128 random bits (22 base64url characters).
    #[schema(example = "https://cardsharing.app/i/3q2-7wAAAAAAAAAAAAAAAA")]
    pub url: String,
}

impl From<MyInvite> for InviteResponse {
    fn from(invite: MyInvite) -> Self {
        Self { url: invite.url }
    }
}

/// Get (or create) my invite link. Auth: ses.
#[utoipa::path(
    get,
    path = "/v1/me/invite",
    tag = TAG,
    operation_id = "getMyInvite",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The caller's live invite link (the same until reset).", body = InviteResponse),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn get_my_invite(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<InviteResponse>, ApiError> {
    let invite = state
        .social
        .my_invite(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(invite.into()))
}

/// Reset my invite link: the old link answers "invite expired" from now on. Auth: ses.
#[utoipa::path(
    post,
    path = "/v1/me/invite/reset",
    tag = TAG,
    operation_id = "resetMyInvite",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The new invite link.", body = InviteResponse),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn reset_my_invite(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<InviteResponse>, ApiError> {
    let invite = state
        .social
        .reset_invite(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(invite.into()))
}
