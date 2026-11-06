//! Board invitations (T-106, AC-19.1, AC-19.8, SR-21): membership requires the invitee's
//! acceptance. Declines and silent drops (a block between the invitee and a member) look pending
//! to the owner until the invitation expires.

use axum::Json;
use axum::extract::{Path, State};
use boards::{InviteOutcome, OwnerInvitation};
use http::StatusCode;
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::routes::boards::{TAG, board_error, board_id};
use crate::state::AppState;

/// Body of `POST /v1/boards/{id}/invitations`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InviteRequest {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: String,
}

/// An invitation as the owner sees it: always `pending` until it is accepted, cancelled or
/// expires (declines and drops are not revealed).
#[derive(Debug, Serialize, ToSchema)]
pub struct OwnerInvitationView {
    #[schema(format = "uuid")]
    pub id: String,
    #[schema(format = "uuid")]
    pub invitee_id: String,
    pub invitee_display_name: Option<String>,
    /// Always `pending`.
    pub status: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct InvitationEnvelope {
    pub invitation: OwnerInvitationView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OwnerInvitationList {
    /// Oldest first.
    pub items: Vec<OwnerInvitationView>,
}

/// A pending invitation as its invitee sees it. No member list before accepting.
#[derive(Debug, Serialize, ToSchema)]
pub struct ReceivedInvitationView {
    #[schema(format = "uuid")]
    pub id: String,
    #[schema(format = "uuid")]
    pub board_id: String,
    pub board_title: String,
    pub owner_display_name: Option<String>,
    /// Members including the owner.
    pub member_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReceivedInvitationList {
    /// Newest first.
    pub items: Vec<ReceivedInvitationView>,
}

/// `{board_id}` of the board just joined.
#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptedInvitation {
    #[schema(format = "uuid")]
    pub board_id: String,
}

/// A malformed id is just another id that does not exist.
fn object_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw).map_err(|_| ApiError::NotFound)
}

async fn owner_views(
    state: &AppState,
    invitations: Vec<OwnerInvitation>,
) -> Result<Vec<OwnerInvitationView>, ApiError> {
    let ids: Vec<Uuid> = invitations.iter().map(|i| i.invitee_id).collect();
    let profiles = state
        .identity
        .public_profiles(&ids)
        .await
        .map_err(ApiError::internal)?;
    Ok(invitations
        .into_iter()
        .map(|invitation| OwnerInvitationView {
            id: invitation.id.hyphenated().to_string(),
            invitee_id: invitation.invitee_id.hyphenated().to_string(),
            invitee_display_name: profiles
                .get(&invitation.invitee_id)
                .map(|p| p.display_name.clone()),
            status: "pending",
            expires_at: invitation.expires_at,
        })
        .collect())
}

/// Invite a friend to a board. Auth: ses. Owner only. Requires `Idempotency-Key`.
///
/// The invitee must be the owner's current friend (no block) and not a member yet; members plus
/// open invitations stay within 8. If the invitee and any member block each other, the answer is
/// the same 201 but nothing reaches the invitee. Invitations expire after 7 days. Re-inviting
/// after an unanswered invitation waits 7 days after its expiry. A board the caller does not own,
/// and a target who is not their friend, are the missing 404.
#[utoipa::path(
    post,
    path = "/v1/boards/{id}/invitations",
    tag = TAG,
    operation_id = "createBoardInvitation",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The board id.")),
    request_body = InviteRequest,
    responses(
        (status = CREATED, description = "Invited (also when silently dropped).", body = InvitationEnvelope),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED, or a missing Idempotency-Key.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no board of the caller, or the target is not their friend.", body = ProblemDetails),
        (status = CONFLICT, description = "ALREADY_INVITED (member or open invitation) or COLLABORATOR_LIMIT_REACHED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED or REINVITE_COOLDOWN (with Retry-After).", body = ProblemDetails),
    )
)]
pub async fn create_board_invitation(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    body: ValidJson<InviteRequest>,
) -> Result<(StatusCode, Json<InvitationEnvelope>), ApiError> {
    let board = board_id(&id)?;
    let invitee = object_id(&body.0.user_id)?;
    let outcome = state
        .boards
        .invite(board, caller.0.account_id, invitee)
        .await
        .map_err(board_error)?;
    match outcome {
        InviteOutcome::Invited(invitation) => {
            let mut views = owner_views(&state, vec![invitation]).await?;
            let invitation = views.pop().ok_or(ApiError::NotFound)?;
            Ok((StatusCode::CREATED, Json(InvitationEnvelope { invitation })))
        }
        InviteOutcome::AlreadyInvited => Err(ApiError::AlreadyInvited),
        InviteOutcome::LimitReached => Err(ApiError::CollaboratorLimitReached),
        InviteOutcome::Cooldown { retry_after_secs } => {
            Err(ApiError::ReinviteCooldown { retry_after_secs })
        }
    }
}

/// Open invitations of my board. Auth: ses. Owner only.
#[utoipa::path(
    get,
    path = "/v1/boards/{id}/invitations",
    tag = TAG,
    operation_id = "listBoardInvitations",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The board id.")),
    responses(
        (status = OK, description = "Open invitations (declined and dropped ones look pending until they expire).", body = OwnerInvitationList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no board of the caller with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_board_invitations(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<OwnerInvitationList>, ApiError> {
    let invitations = state
        .boards
        .owner_invitations(board_id(&id)?, caller.0.account_id)
        .await
        .map_err(board_error)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(OwnerInvitationList {
        items: owner_views(&state, invitations).await?,
    }))
}

/// Cancel an open invitation I sent. Auth: ses.
#[utoipa::path(
    delete,
    path = "/v1/board-invitations/{id}",
    tag = TAG,
    operation_id = "cancelBoardInvitation",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The invitation id.")),
    responses(
        (status = NO_CONTENT, description = "Cancelled."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no open invitation sent by the caller.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn cancel_board_invitation(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let cancelled = state
        .boards
        .cancel_invitation(object_id(&id)?, caller.0.account_id)
        .await
        .map_err(board_error)?;
    if cancelled {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// Board invitations I received. Auth: ses.
///
/// Pending, unexpired invitations to live boards: board title, owner name, member count and
/// expiry. The member list is only visible after accepting.
#[utoipa::path(
    get,
    path = "/v1/me/board-invitations",
    tag = TAG,
    operation_id = "listMyBoardInvitations",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The caller's pending invitations.", body = ReceivedInvitationList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_my_board_invitations(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<ReceivedInvitationList>, ApiError> {
    let invitations = state
        .boards
        .received_invitations(caller.0.account_id)
        .await
        .map_err(board_error)?;
    let owners: Vec<Uuid> = invitations.iter().map(|i| i.owner_id).collect();
    let profiles = state
        .identity
        .public_profiles(&owners)
        .await
        .map_err(ApiError::internal)?;
    let items = invitations
        .into_iter()
        .map(|invitation| ReceivedInvitationView {
            id: invitation.id.hyphenated().to_string(),
            board_id: invitation.board_id.hyphenated().to_string(),
            board_title: invitation.board_title,
            owner_display_name: profiles
                .get(&invitation.owner_id)
                .map(|p| p.display_name.clone()),
            member_count: invitation.member_count,
            expires_at: invitation.expires_at,
        })
        .collect();
    Ok(Json(ReceivedInvitationList { items }))
}

/// Accept a board invitation. Auth: ses (+ device attestation).
///
/// Re-checks in one transaction: the invitation is mine, pending and unexpired; I am still the
/// owner's friend; no member and I block each other; the collaborator cap holds. Any failure,
/// and anyone else's invitation id, is the missing 404.
#[utoipa::path(
    post,
    path = "/v1/board-invitations/{id}/accept",
    tag = TAG,
    operation_id = "acceptBoardInvitation",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The invitation id.")),
    responses(
        (status = OK, description = "Joined the board as a collaborator.", body = AcceptedInvitation),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE, ACCOUNT_SUSPENDED or ATTESTATION_FAILED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no invitation the caller can accept.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn accept_board_invitation(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<AcceptedInvitation>, ApiError> {
    let board = state
        .boards
        .accept_invitation(object_id(&id)?, caller.0.account_id)
        .await
        .map_err(board_error)?;
    Ok(Json(AcceptedInvitation {
        board_id: board.hyphenated().to_string(),
    }))
}

/// Decline a board invitation. Auth: ses. Silent to the owner.
#[utoipa::path(
    post,
    path = "/v1/board-invitations/{id}/decline",
    tag = TAG,
    operation_id = "declineBoardInvitation",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The invitation id.")),
    responses(
        (status = NO_CONTENT, description = "Declined."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no pending invitation of the caller.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn decline_board_invitation(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let declined = state
        .boards
        .decline_invitation(object_id(&id)?, caller.0.account_id)
        .await
        .map_err(board_error)?;
    if declined {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
