//! Friend requests (T-39, T-40, US-5, SR-20, SR-22, SR-13): preview an invite, send a request from
//! it, list, accept, decline and cancel. The sender is always the caller; the recipient comes from
//! the invite token, never from an id. Answering a request that is not the caller's is the same
//! 404 as a missing id.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use http::{HeaderMap, StatusCode};
use platform::problem::{ApiError, ProblemDetails};
use policy::PolicyContext;
use serde::{Deserialize, Serialize};
use social::{
    AnswerError, Direction, FriendSummary, InviteError, InvitePreview, RequestError, RequestHint,
    RequestItem, SentRequest,
};
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
        RequestError::RefNotFound => ApiError::NotFound,
        RequestError::SelfRequest => {
            ApiError::Validation("cannot send a friend request to yourself".to_owned())
        }
        RequestError::AlreadyFriends => ApiError::AlreadyFriends,
        RequestError::FriendLimit => ApiError::FriendLimitReached,
        RequestError::RequestLimit { retry_after_secs } => {
            ApiError::RateLimited { retry_after_secs }
        }
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
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed; retry after Retry-After).", body = ProblemDetails),
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

/// Body of `POST /v1/friend-requests`: exactly one of `invite_token` and `search_ref`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateFriendRequest {
    /// Token of the invite the caller opened.
    pub invite_token: Option<String>,
    /// `ref` of a handle-search result the caller got (10 minutes, caller only).
    pub search_ref: Option<String>,
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

/// Send a friend request to the owner of an invite or to a handle-search result. Auth: ses + att;
/// `Idempotency-Key` required.
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
        (status = NOT_FOUND, description = "NOT_FOUND (unknown invite; foreign, forged or expired search ref) or INVITE_EXPIRED.", body = ProblemDetails),
        (status = CONFLICT, description = "ALREADY_FRIENDS, FRIEND_LIMIT_REACHED or IDEMPOTENCY_IN_PROGRESS.", body = ProblemDetails),
        (status = UNPROCESSABLE_ENTITY, description = "IDEMPOTENCY_KEY_REUSED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (also: 20 open requests or 30 sent in 24 hours; Retry-After).", body = ProblemDetails),
    )
)]
pub async fn create_friend_request(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<CreateFriendRequest>,
) -> Result<(StatusCode, Json<CreateFriendRequestResponse>), ApiError> {
    let caller = caller.0.account_id;
    let sent = match (body.0.invite_token, body.0.search_ref) {
        (Some(token), None) => state.social.request_from_invite(caller, &token).await,
        (None, Some(search_ref)) => state.social.request_from_search(caller, &search_ref).await,
        _ => {
            return Err(ApiError::Validation(
                "exactly one of invite_token and search_ref is required".to_owned(),
            ));
        }
    }
    .map_err(request_error)?;
    Ok((StatusCode::CREATED, Json(sent.into())))
}

fn answer_error(err: AnswerError) -> ApiError {
    match err {
        AnswerError::NotFound => ApiError::NotFound,
        AnswerError::FriendLimit => ApiError::FriendLimitReached,
        AnswerError::Social(err) => ApiError::internal(err),
    }
}

/// A malformed id is just another id that does not exist.
fn request_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw).map_err(|_| ApiError::NotFound)
}

/// Another account as shown in request and friend lists.
#[derive(Debug, Serialize, ToSchema)]
pub struct UserSummaryView {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(example = "Mai")]
    pub display_name: String,
    #[schema(example = "mai.cat")]
    pub handle: Option<String>,
}

impl From<FriendSummary> for UserSummaryView {
    fn from(summary: FriendSummary) -> Self {
        Self {
            id: summary.id,
            display_name: summary.display_name,
            handle: summary.handle,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DirectionParam {
    Incoming,
    Outgoing,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub direction: DirectionParam,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestItemView {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// The sender (incoming) or the recipient (outgoing).
    pub user: UserSummaryView,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
    /// Present only on incoming requests to a minor whose sender is an adult with no mutual
    /// friends (AC-5.5). Never present for adult recipients.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(inline)]
    pub hint: Option<HintView>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HintView {
    AdultNoMutualFriends,
}

impl From<RequestItem> for RequestItemView {
    fn from(item: RequestItem) -> Self {
        Self {
            id: item.id,
            user: item.other.into(),
            created_at: item.created_at,
            expires_at: item.expires_at,
            hint: item.hint.map(|hint| match hint {
                RequestHint::AdultNoMutualFriends => HintView::AdultNoMutualFriends,
            }),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestList {
    /// Newest first, at most 100.
    pub items: Vec<RequestItemView>,
}

/// My pending friend requests. Auth: ses.
///
/// `incoming`: requests addressed to me that I have not answered; for a minor, a request from an
/// adult with no mutual friends carries `hint: "adult_no_mutual_friends"`. `outgoing`: requests I
/// sent that are still open; they all show as pending (declines are silent).
#[utoipa::path(
    get,
    path = "/v1/friend-requests",
    tag = TAG,
    operation_id = "listFriendRequests",
    security(("bearer" = [])),
    params(("direction" = inline(DirectionParam), Query, description = "`incoming` or `outgoing`.")),
    responses(
        (status = OK, description = "Open requests, newest first.", body = RequestList),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (missing or unknown direction).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_friend_requests(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<RequestList>, ApiError> {
    let Query(query) = query
        .map_err(|_| ApiError::Validation("direction must be incoming or outgoing".to_owned()))?;
    let direction = match query.direction {
        DirectionParam::Incoming => Direction::Incoming,
        DirectionParam::Outgoing => Direction::Outgoing,
    };
    let items = state
        .social
        .list_requests(
            caller.0.account_id,
            direction,
            &PolicyContext {
                ip_country: state.ip_country(&headers),
                ..PolicyContext::default()
            },
        )
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(RequestList {
        items: items.into_iter().map(RequestItemView::from).collect(),
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptResponse {
    /// The new friend.
    pub friend: UserSummaryView,
}

/// Accept a friend request addressed to me. Auth: ses + att.
///
/// The 20-friend cap is re-checked for both sides at accept time; when either is full the request
/// stays pending.
#[utoipa::path(
    post,
    path = "/v1/friend-requests/{id}/accept",
    tag = TAG,
    operation_id = "acceptFriendRequest",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "Request id from the incoming list.")),
    responses(
        (status = OK, description = "Now friends.", body = AcceptResponse),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE, ACCOUNT_SUSPENDED or ATTESTATION_FAILED (enforce mode).", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no pending request to the caller with this id.", body = ProblemDetails),
        (status = CONFLICT, description = "FRIEND_LIMIT_REACHED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn accept_friend_request(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<AcceptResponse>, ApiError> {
    let friend = state
        .social
        .accept_request(caller.0.account_id, request_id(&id)?)
        .await
        .map_err(answer_error)?;
    Ok(Json(AcceptResponse {
        friend: friend.into(),
    }))
}

/// Decline a friend request addressed to me. Auth: ses. Silent: the sender is not told.
#[utoipa::path(
    post,
    path = "/v1/friend-requests/{id}/decline",
    tag = TAG,
    operation_id = "declineFriendRequest",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "Request id from the incoming list.")),
    responses(
        (status = NO_CONTENT, description = "Declined."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no pending request to the caller with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn decline_friend_request(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .social
        .decline_request(caller.0.account_id, request_id(&id)?)
        .await
        .map_err(answer_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Cancel a friend request I sent. Auth: ses.
#[utoipa::path(
    delete,
    path = "/v1/friend-requests/{id}",
    tag = TAG,
    operation_id = "cancelFriendRequest",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "Request id from the outgoing list.")),
    responses(
        (status = NO_CONTENT, description = "Cancelled."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no open request from the caller with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn cancel_friend_request(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .social
        .cancel_request(caller.0.account_id, request_id(&id)?)
        .await
        .map_err(answer_error)?;
    Ok(StatusCode::NO_CONTENT)
}
