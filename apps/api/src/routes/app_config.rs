//! `GET /v1/config` (T-27): public client configuration. Identical for every caller, so it is
//! CDN cacheable and must never contain user data.

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, header};
use platform::problem::ProblemDetails;

use crate::client_config::AppConfigResponse;
use crate::state::AppState;

pub const TAG: &str = "config";

/// Limits, feature flags and registry kill switches. Auth: none.
#[utoipa::path(
    get,
    path = "/v1/config",
    tag = TAG,
    operation_id = "getConfig",
    security(()),
    responses(
        (status = OK, description = "Public client configuration (cacheable, no user data).", body = AppConfigResponse),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn get_config(State(state): State<AppState>) -> Response {
    let mut response = Json(state.client.response.clone()).into_response();
    let cache = format!("public, max-age={}", state.client.cache_max_age_secs);
    if let Ok(value) = HeaderValue::from_str(&cache) {
        response.headers_mut().insert(header::CACHE_CONTROL, value);
    }
    response
}
