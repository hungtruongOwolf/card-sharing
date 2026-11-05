//! Friend requests (T-39, US-5, SR-20, SR-22, SR-13): preview an invite, send a request from it.
//! The sender is always the caller; the recipient comes from the invite token, never from an id.

use axum::Json;
use axum::extract::{Path, State};
use http::StatusCode;
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use social::{InviteError, InvitePreview, RequestError, SentRequest};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::routes::invites::TAG;
use crate::state::AppState;

fn invite_error(err: InviteError) -> ApiError {
    match err {
        InviteError::NotFound => ApiError::NotFound,
        InviteError::Expired => ApiError::InviteExpired,
        InviteError::Social(err) => ApiError::internal(err),
    }
}

fn request_error(err: RequestError) -> ApiError {
    match err {
        RequestError::InviteNotFound => ApiError::NotFound,
        RequestError::InviteExpired => ApiError::InviteExpired,
        RequestError::SelfRequest => {
            ApiError::Validation("cannot send a friend request to yourself".to_owned())
        }
        RequestError::AlreadyFriends => ApiError::AlreadyFriends,
        RequestError::FriendLimit => ApiError::FriendLimitReached,
        RequestError::Social(err) => ApiError::internal(err),
    }
}

/// The inviter as shown before sending a request.
#[derive(Debug, Serialize, ToSchema)]
pub struct InviterView {
    #[schema(example = "Mai")]
    pub display_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct InvitePreviewResponse {
    pub inviter: InviterView,
    /// The caller opened their own invite ("That's you!"); sending a request is refused.
    pub is_self: bool,
}

impl From<InvitePreview> for InvitePreviewResponse {
    fn from(preview: InvitePreview) -> Self {
        Self {
            inviter: InviterView {
                display_name: preview.display_name,
            },
            is_self: preview.is_self,
        }
    }
}

/// Preview an invite before adding the inviter. Auth: ses; rate-limited per client IP.
///
/// Unknown tokens, suspended inviters and blocks (either way) all answer the same 404
/// `NOT_FOUND`; a reset, expired or used-up invite answers 404 `INVITE_EXPIRED`.
#[utoipa::path(
    get,
    path = "/v1/invites/{token}",
    tag = TAG,
    operation_id = "previewInvite",
    security(("bearer" = [])),
    params(("token" = String, Path, description = "The token part of an invite link (22 characters).")),
    responses(
        (status = OK, description = "The inviter's display name (no avatar for a minor's invite).", body = InvitePreviewResponse),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND (unknown) or INVITE_EXPIRED (reset, expired or used up).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE (rate limit store down; fails closed).", body = ProblemDetails),
    )
)]
pub async fn preview_invite(
    State(state): State<AppState>,
    caller: Caller,
    Path(token): Path<String>,
) -> Result<Json<InvitePreviewResponse>, ApiError> {
    let preview = state
        .social
        .invite_preview(caller.0.account_id, &token)
        .await
        .map_err(invite_error)?;
    Ok(Json(preview.into()))
}

/// Body of `POST /v1/friend-requests`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateFriendRequest {
    /// Token of the invite the caller opened.
    pub invite_token: String,
}

/// A request as its sender sees it: pending until accepted or expired.
#[derive(Debug, Serialize, ToSchema)]
pub struct SentRequestView {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// Always `pending` for the sender (declines and blocks are silent).
    #[schema(example = "pending")]
    pub status: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateFriendRequestResponse {
    pub request: SentRequestView,
}

impl From<SentRequest> for CreateFriendRequestResponse {
    fn from(sent: SentRequest) -> Self {
        Self {
            request: SentRequestView {
                id: sent.id,
                status: "pending",
                expires_at: sent.expires_at,
            },
        }
    }
}

/// Send a friend request to the owner of an invite. Auth: ses + att; `Idempotency-Key` required.
///
/// The inviter must accept before a friendship exists. A request between accounts with a block
/// (either way) gets the same 201 and is never delivered. Repeating a request returns the open
/// one.
#[utoipa::path(
    post,
    path = "/v1/friend-requests",
    tag = TAG,
    operation_id = "createFriendRequest",
    security(("bearer" = [])),
    request_body = CreateFriendRequest,
    responses(
        (status = CREATED, description = "Sent (or already open). Identical for silently dropped requests.", body = CreateFriendRequestResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (also: your own invite).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE, ACCOUNT_SUSPENDED or ATTESTATION_FAILED (enforce mode).", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND or INVITE_EXPIRED.", body = ProblemDetails),
        (status = CONFLICT, description = "ALREADY_FRIENDS, FRIEND_LIMIT_REACHED or IDEMPOTENCY_IN_PROGRESS.", body = ProblemDetails),
        (status = UNPROCESSABLE_ENTITY, description = "IDEMPOTENCY_KEY_REUSED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn create_friend_request(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<CreateFriendRequest>,
) -> Result<(StatusCode, Json<CreateFriendRequestResponse>), ApiError> {
    let sent = state
        .social
        .request_from_invite(caller.0.account_id, &body.0.invite_token)
        .await
        .map_err(request_error)?;
    Ok((StatusCode::CREATED, Json(sent.into())))
}
