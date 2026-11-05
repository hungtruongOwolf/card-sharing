//! Exact-handle search (T-43, US-5a, SR-19, SR-23, SR-13). Every answer that depends on the
//! target (found, missing, hidden, errors) is padded to a multiple of the configured latency floor,
//! and a hidden account gets the byte-identical "no one found" body of a missing handle.

use std::time::Duration;

use axum::Json;
use axum::extract::{Extension, State};
use platform::problem::{ApiError, ProblemDetails};
use platform::rate_limit::{AccountSubject, Decision, Subject};
use serde::{Deserialize, Serialize};
use social::SearchHit;
use tokio::time::Instant;
use utoipa::ToSchema;

use crate::access::{Caller, ClientIp, ValidJson};
use crate::routes::invites::TAG;
use crate::state::AppState;

/// Route limit (per account): 10/min and 30/day.
pub const USER_LIMIT: &str = "users.search";
/// Extra windows of the same route, keyed by device and by client IP (AC-5a.3).
const DEVICE_LIMIT: &str = "users.search.device";
const IP_LIMIT: &str = "users.search.ip";

/// Body of `POST /v1/users/search`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    /// The exact handle, case-insensitive, with or without a leading `@`. Partial input never
    /// matches anything.
    #[schema(example = "@mai.cat")]
    pub handle: String,
}

/// The one account found.
#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResultView {
    /// Pass as `search_ref` to `POST /v1/friend-requests` within 10 minutes. Opaque, and only valid
    /// for the account that searched.
    #[serde(rename = "ref")]
    pub search_ref: String,
    #[schema(example = "Mai")]
    pub display_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResponse {
    /// `null` = "No one found": the same for a missing handle and for any hidden account.
    pub result: Option<SearchResultView>,
}

impl From<Option<SearchHit>> for SearchResponse {
    fn from(hit: Option<SearchHit>) -> Self {
        Self {
            result: hit.map(|hit| SearchResultView {
                search_ref: hit.search_ref,
                display_name: hit.display_name,
            }),
        }
    }
}

/// Find a friend by exact handle. Auth: ses + att.
///
/// At most one result, only on an exact case-insensitive match. Accounts under the region's
/// discovery age, accounts that turned "Findable by handle" off, blocks either way and your own
/// handle all answer exactly like a handle nobody has (same body, same latency class). Limited
/// per account (10/min, 30/day), per device and per IP; over a limit the answer is a generic
/// "try later" (429).
#[utoipa::path(
    post,
    path = "/v1/users/search",
    tag = TAG,
    operation_id = "searchUsers",
    security(("bearer" = [])),
    request_body = SearchRequest,
    responses(
        (status = OK, description = "`{result: null}` or the one account found.", body = SearchResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (body shape only; any string is searched).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE, ACCOUNT_SUSPENDED or ATTESTATION_FAILED (enforce mode).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (try later).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE (rate limit store down; fails closed).", body = ProblemDetails),
    )
)]
pub async fn search_users(
    State(state): State<AppState>,
    caller: Caller,
    Extension(client_ip): Extension<ClientIp>,
    body: ValidJson<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiError> {
    let subject = Subject {
        account: Some(AccountSubject {
            id: caller.0.account_id,
            created_at: caller.0.account_created_at,
        }),
        device: caller.0.device_id,
        ip: client_ip.0,
    };
    // These answers depend only on the caller, so they are not padded.
    for limit in [DEVICE_LIMIT, IP_LIMIT] {
        check_limit(&state, limit, &subject).await?;
    }
    let started = Instant::now();
    let outcome = state
        .social
        .search_by_handle(caller.0.account_id, &body.0.handle)
        .await;
    let floor = Duration::from_millis(u64::from(state.social.settings().search_latency_floor_ms));
    pad_to_bucket(started, floor).await;
    let hit = outcome.map_err(ApiError::internal)?;
    Ok(Json(hit.into()))
}

async fn check_limit(state: &AppState, limit: &str, subject: &Subject) -> Result<(), ApiError> {
    match state.limiter.check(limit, subject).await {
        Ok(Decision::Allowed) => Ok(()),
        Ok(Decision::Limited { retry_after_secs }) => {
            Err(ApiError::RateLimited { retry_after_secs })
        }
        Ok(Decision::Unavailable) => Err(ApiError::Unavailable {
            retry_after_secs: 5,
        }),
        Err(err) => Err(ApiError::internal(err)),
    }
}

/// Sleeps until the next multiple of `floor` after `started` (at least one `floor`).
async fn pad_to_bucket(started: Instant, floor: Duration) {
    let elapsed = started.elapsed();
    let buckets = elapsed.as_nanos() / floor.as_nanos().max(1) + 1;
    let target = floor.saturating_mul(u32::try_from(buckets).unwrap_or(u32::MAX));
    tokio::time::sleep_until(started + target).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn answers_are_padded_to_the_next_bucket() {
        let floor = Duration::from_millis(20);
        let started = Instant::now();
        pad_to_bucket(started, floor).await;
        let quick = started.elapsed();
        assert!(quick >= floor && quick < floor * 2, "{quick:?}");

        let started = Instant::now();
        tokio::time::sleep(Duration::from_millis(25)).await;
        pad_to_bucket(started, floor).await;
        assert!(started.elapsed() >= floor * 2, "{:?}", started.elapsed());
    }
}
