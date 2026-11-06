//! Activity tab (T-88, US-31 AC-31.3): every alert-worthy event, push or not. Only the caller's
//! own items, re-checked for visibility on every read; ids only (the app resolves cards, requests
//! and people through their own routes). Cursor pagination with a server-signed cursor.

use axum::Json;
use axum::extract::State;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::{StatusCode, Uri};
use notifications::ActivityItem;
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::state::AppState;

pub const TAG: &str = "activity";
pub const ACTIVITY_PAGE_MAX: i64 = 50;
const ACTIVITY_PAGE_DEFAULT: i64 = 30;
/// The badge counts up to this many unread items ("99+" beyond).
pub const UNREAD_CAP: i64 = 100;
const CURSOR_PURPOSE: &str = "activity_cursor";

#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ActivityParams {
    /// `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// 1..=50, default 30.
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ActivityItemView {
    /// Item id; pass the newest one seen to `POST /v1/activity/read`.
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// `card.new`, `reaction.new`, `friend.request`, `friend.accepted` or `card.held`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The card (`card.*`, `reaction.new`), friend request (`friend.request`) or user
    /// (`friend.accepted`) the item is about.
    #[schema(value_type = String, format = "uuid")]
    pub subject_id: Uuid,
    /// Card sender, reactor or new friend; `null` when the event names nobody.
    #[schema(value_type = Option<String>, format = "uuid")]
    pub actor_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
    pub unread: bool,
}

impl From<ActivityItem> for ActivityItemView {
    fn from(item: ActivityItem) -> Self {
        Self {
            id: item.id,
            kind: item.kind,
            subject_id: item.subject_id,
            actor_id: item.actor_id,
            created_at: item.created_at,
            unread: item.unread,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ActivityPage {
    /// Newest first.
    pub items: Vec<ActivityItemView>,
    /// Unread visible items, counted up to 100.
    pub unread_count: i64,
    /// Opaque; `null` on the last page.
    pub next_cursor: Option<String>,
}

/// Body of `POST /v1/activity/read`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkReadRequest {
    /// The newest item the user has seen; it and everything older become read.
    #[schema(value_type = String, format = "uuid")]
    pub up_to: Uuid,
}

fn signer(state: &AppState) -> Result<&media::UrlSigner, ApiError> {
    state.media_signer.as_deref().ok_or(ApiError::Unavailable {
        retry_after_secs: 30,
    })
}

fn encode_cursor(state: &AppState, id: Uuid) -> Result<String, ApiError> {
    let mut data = id.as_bytes().to_vec();
    data.extend_from_slice(&signer(state)?.token_tag(CURSOR_PURPOSE, id.as_bytes()));
    Ok(URL_SAFE_NO_PAD.encode(data))
}

fn decode_cursor(state: &AppState, raw: &str) -> Result<Uuid, ApiError> {
    let invalid = || ApiError::Validation("cursor is invalid".to_owned());
    let signer = signer(state)?;
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    if bytes.len() != 16 + 32 {
        return Err(invalid());
    }
    let (data, tag) = bytes.split_at(16);
    if !signer.verify_token_tag(CURSOR_PURPOSE, data, tag) {
        return Err(invalid());
    }
    Uuid::from_slice(data).map_err(|_| invalid())
}

/// My activity. Auth: ses.
///
/// Every new card, reaction, friend request, accepted request and review hold, also when no
/// push was sent (AC-31.3). Items the caller may no longer see (unfriended, blocked, deleted,
/// answered requests) are left out.
#[utoipa::path(
    get,
    path = "/v1/activity",
    tag = TAG,
    operation_id = "getActivity",
    security(("bearer" = [])),
    params(ActivityParams),
    responses(
        (status = OK, description = "One page of activity.", body = ActivityPage),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: bad limit or cursor.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: cursor signing not configured.", body = ProblemDetails),
    )
)]
pub async fn get_activity(
    State(state): State<AppState>,
    caller: Caller,
    uri: Uri,
) -> Result<Json<ActivityPage>, ApiError> {
    let params = axum::extract::Query::<ActivityParams>::try_from_uri(&uri)
        .map_err(|_| ApiError::Validation("query must be ?cursor=&limit=".to_owned()))?
        .0;
    let limit = params.limit.unwrap_or(ACTIVITY_PAGE_DEFAULT);
    if !(1..=ACTIVITY_PAGE_MAX).contains(&limit) {
        return Err(ApiError::Validation(format!(
            "limit must be between 1 and {ACTIVITY_PAGE_MAX}"
        )));
    }
    let before = params
        .cursor
        .as_deref()
        .map(|raw| decode_cursor(&state, raw))
        .transpose()?;
    let account = caller.0.account_id;
    let mut items = state
        .activity
        .page(account, before, limit + 1)
        .await
        .map_err(ApiError::internal)?;
    let limit = usize::try_from(limit).unwrap_or(0);
    let more = items.len() > limit;
    items.truncate(limit);
    let next_cursor = match (more, items.last()) {
        (true, Some(last)) => Some(encode_cursor(&state, last.id)?),
        _ => None,
    };
    let unread_count = state
        .activity
        .unread_count(account, UNREAD_CAP)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(ActivityPage {
        items: items.into_iter().map(ActivityItemView::from).collect(),
        unread_count,
        next_cursor,
    }))
}

/// Mark my activity read up to an item. Auth: ses.
///
/// Never moves backwards. An id that is not one of the caller's items is the same 404 as a
/// missing one.
#[utoipa::path(
    post,
    path = "/v1/activity/read",
    tag = TAG,
    operation_id = "markActivityRead",
    security(("bearer" = [])),
    request_body = MarkReadRequest,
    responses(
        (status = NO_CONTENT, description = "Marked read."),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: not one of the caller's items.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn mark_activity_read(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<MarkReadRequest>,
) -> Result<StatusCode, ApiError> {
    let marked = state
        .activity
        .mark_read(caller.0.account_id, body.0.up_to)
        .await
        .map_err(ApiError::internal)?;
    if marked {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
