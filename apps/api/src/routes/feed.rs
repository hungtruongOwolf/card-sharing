//! Feed (US-15, part of #82): cards delivered to me, newest delivery first, through the same
//! visibility predicate as a card read (still friends, no block, card active, sender active).
//! Media are per-viewer signed URLs. Pagination uses an opaque, server-signed cursor.

use axum::Json;
use axum::extract::State;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::Uri;
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::access::Caller;
use crate::routes::cards::{CardView, TAG, card_views};
use crate::state::AppState;

/// Largest page (AC-15.1: 30 per page).
pub const FEED_PAGE_MAX: i64 = 30;
const CURSOR_PURPOSE: &str = "feed_cursor";

#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct FeedParams {
    /// `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// 1..=30, default 30.
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FeedPage {
    pub items: Vec<CardView>,
    /// Opaque; `null` on the last page.
    pub next_cursor: Option<String>,
}

fn encode_cursor(state: &AppState, at: OffsetDateTime, id: Uuid) -> Result<String, ApiError> {
    let signer = state.media_signer.as_ref().ok_or(ApiError::Unavailable {
        retry_after_secs: 30,
    })?;
    let micros = i64::try_from(at.unix_timestamp_nanos() / 1000).map_err(ApiError::internal)?;
    let mut data = micros.to_be_bytes().to_vec();
    data.extend_from_slice(id.as_bytes());
    let tag = signer.token_tag(CURSOR_PURPOSE, &data);
    data.extend_from_slice(&tag);
    Ok(URL_SAFE_NO_PAD.encode(data))
}

fn decode_cursor(state: &AppState, raw: &str) -> Result<(OffsetDateTime, Uuid), ApiError> {
    let invalid = || ApiError::Validation("cursor is invalid".to_owned());
    let signer = state.media_signer.as_ref().ok_or(ApiError::Unavailable {
        retry_after_secs: 30,
    })?;
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    if bytes.len() != 8 + 16 + 32 {
        return Err(invalid());
    }
    let (data, tag) = bytes.split_at(24);
    if !signer.verify_token_tag(CURSOR_PURPOSE, data, tag) {
        return Err(invalid());
    }
    let mut micros = [0u8; 8];
    micros.copy_from_slice(&data[..8]);
    let at =
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(i64::from_be_bytes(micros)) * 1000)
            .map_err(|_| invalid())?;
    let id = Uuid::from_slice(&data[8..]).map_err(|_| invalid())?;
    Ok((at, id))
}

/// My feed. Auth: ses.
///
/// Cards delivered to the caller by current friends (no block either way, card active, sender
/// active), newest delivery first, `limit` <= 30. Unfriending, blocking or deleting removes a
/// card on the next read. Media are signed for the caller (10 min).
#[utoipa::path(
    get,
    path = "/v1/feed",
    tag = TAG,
    operation_id = "getFeed",
    security(("bearer" = [])),
    params(FeedParams),
    responses(
        (status = OK, description = "One page of the feed.", body = FeedPage),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: bad limit or cursor.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (requests or the signed-URL budget).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: media signing not configured.", body = ProblemDetails),
    )
)]
pub async fn get_feed(
    State(state): State<AppState>,
    caller: Caller,
    uri: Uri,
) -> Result<Json<FeedPage>, ApiError> {
    let params = axum::extract::Query::<FeedParams>::try_from_uri(&uri)
        .map_err(|_| ApiError::Validation("query must be ?cursor=&limit=".to_owned()))?
        .0;
    let limit = params.limit.unwrap_or(FEED_PAGE_MAX);
    if !(1..=FEED_PAGE_MAX).contains(&limit) {
        return Err(ApiError::Validation(format!(
            "limit must be between 1 and {FEED_PAGE_MAX}"
        )));
    }
    let before = params
        .cursor
        .as_deref()
        .map(|raw| decode_cursor(&state, raw))
        .transpose()?;
    let mut cards = state
        .cards
        .feed(caller.0.account_id, before, limit + 1)
        .await
        .map_err(ApiError::internal)?;
    let more = cards.len() > usize::try_from(limit).unwrap_or(0);
    cards.truncate(usize::try_from(limit).unwrap_or(0));
    let next_cursor = match (more, cards.last()) {
        (true, Some(last)) => {
            let at = last
                .delivered_at
                .ok_or(ApiError::internal("feed item without delivery"))?;
            Some(encode_cursor(&state, at, last.id)?)
        }
        _ => None,
    };
    let items = card_views(&state, &caller.0, cards).await?;
    Ok(Json(FeedPage { items, next_cursor }))
}
