//! Friends (T-46, US-6, SR-4): list my friends, unfriend silently. Only edges that include the
//! caller exist on these routes; any other id is the same 404 as a missing one.

use axum::Json;
use axum::extract::{Path, State};
use http::StatusCode;
use platform::problem::{ApiError, ProblemDetails};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::Caller;
use crate::routes::friend_requests::UserSummaryView;
use crate::routes::invites::TAG;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct FriendList {
    /// Most recent friendship first (at most the friend cap, 20).
    pub items: Vec<UserSummaryView>,
}

/// My friends. Auth: ses.
#[utoipa::path(
    get,
    path = "/v1/friends",
    tag = TAG,
    operation_id = "listFriends",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The caller's friends.", body = FriendList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_friends(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<FriendList>, ApiError> {
    let friends = state
        .social
        .friends(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(FriendList {
        items: friends.into_iter().map(UserSummaryView::from).collect(),
    }))
}

/// Remove a friend. Auth: ses. Silent: the other person gets no notification (AC-6.2).
#[utoipa::path(
    delete,
    path = "/v1/friends/{userId}",
    tag = TAG,
    operation_id = "removeFriend",
    security(("bearer" = [])),
    params(("userId" = String, Path, format = "uuid", description = "The friend's account id.")),
    responses(
        (status = NO_CONTENT, description = "No longer friends."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: the caller has no friend with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn remove_friend(
    State(state): State<AppState>,
    caller: Caller,
    Path(user_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let other = Uuid::try_parse(&user_id).map_err(|_| ApiError::NotFound)?;
    let removed = state
        .social
        .unfriend(caller.0.account_id, other)
        .await
        .map_err(ApiError::internal)?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
