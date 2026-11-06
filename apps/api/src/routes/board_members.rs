//! Board members (T-102, AC-19.9, SR-21): `GET /v1/boards/{id}/members`.

use axum::Json;
use axum::extract::{Path, State};
use boards::RemoveOutcome;
use http::StatusCode;
use platform::problem::{ApiError, ProblemDetails};
use serde::Serialize;
use time::OffsetDateTime;
use utoipa::ToSchema;

use crate::access::Caller;
use crate::routes::boards::{TAG, board_error, board_id};
use crate::state::AppState;

/// A member as the caller may see them. An adult caller who is not friends with a minor member
/// gets that member's display name only (no handle).
#[derive(Debug, Serialize, ToSchema)]
pub struct BoardMemberView {
    #[schema(format = "uuid")]
    pub id: String,
    /// `owner` or `collaborator`.
    pub role: &'static str,
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub is_me: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub joined_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BoardMemberList {
    /// The owner first, then collaborators by join time.
    pub items: Vec<BoardMemberView>,
}

/// Members of a board. Auth: ses. Members only.
///
/// An adult caller who is not friends with a minor member sees that member's display name only
/// (AC-19.9). Anyone who is not a member gets the same 404 as for a missing board.
#[utoipa::path(
    get,
    path = "/v1/boards/{id}/members",
    tag = TAG,
    operation_id = "listBoardMembers",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The board id.")),
    responses(
        (status = OK, description = "The board's members.", body = BoardMemberList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no board of which the caller is a member.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_board_members(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<BoardMemberList>, ApiError> {
    let viewer = caller.0.account_id;
    let members = state
        .boards
        .members(board_id(&id)?, viewer)
        .await
        .map_err(board_error)?
        .ok_or(ApiError::NotFound)?;
    let ids: Vec<uuid::Uuid> = members.iter().map(|m| m.user_id).collect();
    let profiles = state
        .identity
        .public_profiles(&ids)
        .await
        .map_err(ApiError::internal)?;
    let items = members
        .into_iter()
        .map(|member| {
            let profile = profiles.get(&member.user_id);
            BoardMemberView {
                id: member.user_id.hyphenated().to_string(),
                role: member.role.as_str(),
                display_name: profile.map(|p| p.display_name.clone()),
                handle: if member.restricted {
                    None
                } else {
                    profile.and_then(|p| p.handle.clone())
                },
                is_me: member.user_id == viewer,
                joined_at: member.joined_at,
            }
        })
        .collect();
    Ok(Json(BoardMemberList { items }))
}

/// Remove a collaborator (owner) or leave a board (self). Auth: ses.
///
/// The member's pins are removed with them and their later writes are refused. The owner cannot
/// leave their own board (delete it instead). A collaborator removing someone else, a
/// non-member, and an unknown user get the same 404 as for a missing board.
#[utoipa::path(
    delete,
    path = "/v1/boards/{id}/members/{userId}",
    tag = TAG,
    operation_id = "removeBoardMember",
    security(("bearer" = [])),
    params(
        ("id" = String, Path, format = "uuid", description = "The board id."),
        ("userId" = String, Path, format = "uuid", description = "The member to remove (the caller's own id to leave)."),
    ),
    responses(
        (status = NO_CONTENT, description = "Removed (or left)."),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: the owner cannot leave their own board.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no such member the caller may remove.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn remove_board_member(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let board = board_id(&id)?;
    let target = uuid::Uuid::try_parse(&user_id).map_err(|_| ApiError::NotFound)?;
    let outcome = state
        .boards
        .remove_member(board, caller.0.account_id, target)
        .await
        .map_err(board_error)?;
    match outcome {
        RemoveOutcome::Removed => Ok(StatusCode::NO_CONTENT),
        RemoveOutcome::OwnerCannotLeave => Err(ApiError::Validation(
            "the owner cannot leave their own board; delete it instead".to_owned(),
        )),
    }
}
