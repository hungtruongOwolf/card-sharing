//! Blocks (T-47, US-26 AC-26.2, SR-22): block, list, unblock. Silent to the blocked account.
//! Blocking an account the caller has no relation to is the same 404 as a missing id.

use axum::Json;
use axum::extract::{Path, State};
use http::StatusCode;
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use social::BlockedAccount;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::routes::friend_requests::UserSummaryView;
use crate::routes::invites::TAG;
use crate::state::AppState;

/// Body of `POST /v1/blocks`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockRequest {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BlockedView {
    pub user: UserSummaryView,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub blocked_at: OffsetDateTime,
}

impl From<BlockedAccount> for BlockedView {
    fn from(blocked: BlockedAccount) -> Self {
        Self {
            user: blocked.user.into(),
            blocked_at: blocked.blocked_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BlockList {
    /// Most recent first.
    pub items: Vec<BlockedView>,
}

/// A malformed id is just another id that does not exist.
fn account_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw).map_err(|_| ApiError::NotFound)
}

/// Block someone. Auth: ses.
///
/// Ends the friendship, drops their pending requests to me, cancels mine to them, and silently
/// refuses future requests both ways (they get the usual answer; nothing is delivered). The
/// account must be related to me (friend or a friend request either way), otherwise 404.
#[utoipa::path(
    post,
    path = "/v1/blocks",
    tag = TAG,
    operation_id = "blockUser",
    security(("bearer" = [])),
    request_body = BlockRequest,
    responses(
        (status = NO_CONTENT, description = "Blocked (also when already blocked)."),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no related account with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn block_user(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<BlockRequest>,
) -> Result<StatusCode, ApiError> {
    let target = account_id(&body.0.user_id)?;
    let blocked = state
        .social
        .block(caller.0.account_id, target)
        .await
        .map_err(ApiError::internal)?;
    if blocked {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// Accounts I blocked. Auth: ses.
#[utoipa::path(
    get,
    path = "/v1/blocks",
    tag = TAG,
    operation_id = "listBlocks",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The caller's blocks.", body = BlockList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_blocks(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<BlockList>, ApiError> {
    let blocked = state
        .social
        .blocked_accounts(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(BlockList {
        items: blocked.into_iter().map(BlockedView::from).collect(),
    }))
}

/// Unblock someone. Auth: ses. The friendship is not restored.
#[utoipa::path(
    delete,
    path = "/v1/blocks/{userId}",
    tag = TAG,
    operation_id = "unblockUser",
    security(("bearer" = [])),
    params(("userId" = String, Path, format = "uuid", description = "The blocked account's id.")),
    responses(
        (status = NO_CONTENT, description = "Unblocked."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: the caller has not blocked this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn unblock_user(
    State(state): State<AppState>,
    caller: Caller,
    Path(user_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let unblocked = state
        .social
        .unblock(caller.0.account_id, account_id(&user_id)?)
        .await
        .map_err(ApiError::internal)?;
    if unblocked {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
