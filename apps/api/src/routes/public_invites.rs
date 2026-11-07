//! Invite landing data for the web share app (#234, T-48, US-4 AC-4.2/4.4, SR-17, SR-20).
//!
//! Called only by the Next.js server with the service credential (`svc`); the visitor's IP comes
//! in `X-Client-IP` and keys the per-IP limit. The response holds only what the landing page shows,
//! and every unusable token answers the same 404 so tokens cannot be probed. The token never
//! reaches logs (spans record the route template only).

use axum::Json;
use axum::extract::{Path, State};
use platform::problem::{ApiError, ProblemDetails};
use serde::Serialize;
use social::{InviteError, PublicInvitePreview};
use utoipa::ToSchema;

use crate::routes::friend_requests::InviterView;
use crate::state::AppState;

pub const TAG: &str = "public";

/// What the invite landing page shows.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicInviteResponse {
    pub inviter: InviterView,
}

impl From<PublicInvitePreview> for PublicInviteResponse {
    fn from(preview: PublicInvitePreview) -> Self {
        Self {
            inviter: InviterView {
                display_name: preview.display_name,
            },
        }
    }
}

/// Invite landing data for `/i/<token>`. Auth: svc (web server only).
///
/// Unknown, malformed, reset, expired and used-up tokens, and invites whose inviter is suspended,
/// banned or deleted, all answer the identical 404 `NOT_FOUND`.
#[utoipa::path(
    get,
    path = "/v1/public/invites/{token}",
    tag = TAG,
    operation_id = "getPublicInvite",
    security(("service" = [])),
    params(
        ("token" = String, Path, description = "The token part of an invite link (22 characters)."),
        ("X-Client-IP" = String, Header, description = "The visitor's IP address as the web server saw it (keys the per-IP limit)."),
    ),
    responses(
        (status = OK, description = "The inviter's display name.", body = PublicInviteResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (X-Client-IP missing or not an IP address).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED (service credential missing or wrong).", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND for every unusable token.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (per visitor IP).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed; retry after Retry-After).", body = ProblemDetails),
    )
)]
pub async fn get_public_invite(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<PublicInviteResponse>, ApiError> {
    let preview = state
        .social
        .public_invite_preview(&token)
        .await
        .map_err(|err| match err {
            InviteError::NotFound | InviteError::Expired => ApiError::NotFound,
            InviteError::Social(err) => ApiError::internal(err),
        })?;
    Ok(Json(preview.into()))
}
