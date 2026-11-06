//! `GET /v1/config` (T-27): public client configuration. Identical for every caller, so it is
//! CDN cacheable and must never contain user data.

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use http::{HeaderMap, HeaderValue, StatusCode, header};
use platform::problem::ProblemDetails;

use crate::client_config::AppConfigResponse;
use crate::state::AppState;

pub const TAG: &str = "config";

/// Limits, feature flags, registry kill switches and the text filter's client hints. Auth: none.
///
/// Cacheable: `Cache-Control: public, max-age=…` plus a strong `ETag`; a matching
/// `If-None-Match` answers 304 with no body.
#[utoipa::path(
    get,
    path = "/v1/config",
    tag = TAG,
    operation_id = "getConfig",
    security(()),
    responses(
        (status = OK, description = "Public client configuration (cacheable, no user data).", body = AppConfigResponse),
        (status = NOT_MODIFIED, description = "The `If-None-Match` tag is current; no body."),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn get_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let client = &state.client;
    let fresh = headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.trim_start_matches("W/") == client.etag);
    let mut response = if fresh {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Json(client.response.clone()).into_response()
    };
    let cache = format!("public, max-age={}", client.cache_max_age_secs);
    if let Ok(value) = HeaderValue::from_str(&cache) {
        response.headers_mut().insert(header::CACHE_CONTROL, value);
    }
    if let Ok(value) = HeaderValue::from_str(&client.etag) {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}
