//! Signed media delivery (T-74, ADR-0005, SR-8): the edge for the `clean` bucket. In production the
//! CDN verifies the same signature in front of R2; locally (and wherever `API_MEDIA_BASE_URL`
//! points here) this route does it. It needs no session: the signature is the capability, and it
//! covers one exact object key, the expiry and the viewer id. Only the `clean` bucket is ever
//! read; quarantine and legal-hold objects are unreachable from any route.

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, Uri, header};
use media::Bucket;
use platform::problem::{ApiError, ProblemDetails};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::routes::media::TAG;
use crate::state::AppState;

/// Query of a signed URL.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SignedQuery {
    /// Expiry (Unix seconds).
    pub exp: Option<String>,
    /// Viewer id the URL was issued to (audit).
    pub v: Option<String>,
    /// HMAC-SHA256 signature (base64url).
    pub sig: Option<String>,
}

/// Fetch a signed media object. Auth: none (the signature is the capability).
///
/// Returns the image bytes (`image/webp`) when the signature over the exact key, expiry and viewer
/// is valid and unexpired; otherwise 403. Clients never build these URLs: they come inside DTOs
/// (with `expires_at`) and are refetched through the DTO after expiry.
#[utoipa::path(
    get,
    path = "/v1/media-files/{key}",
    tag = TAG,
    operation_id = "getMediaFile",
    params(("key" = String, Path, description = "Exact object key."), SignedQuery),
    responses(
        (status = OK, description = "The image.", content_type = "image/webp", body = crate::routes::media::ImageBytes),
        (status = FORBIDDEN, description = "FORBIDDEN: missing, malformed, tampered or expired signature.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: the object no longer exists (deleted card).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: signing or storage not configured.", body = ProblemDetails),
    )
)]
pub async fn get_media_file(
    State(state): State<AppState>,
    Path(key): Path<String>,
    uri: Uri,
) -> Result<Response, ApiError> {
    let signer = state.media_signer.as_ref().ok_or(ApiError::Unavailable {
        retry_after_secs: 30,
    })?;
    let Query(query) = Query::<SignedQuery>::try_from_uri(&uri).map_err(|_| ApiError::Forbidden)?;
    let (Some(exp), Some(viewer), Some(sig)) = (query.exp, query.v, query.sig) else {
        return Err(ApiError::Forbidden);
    };
    let now = state.identity.clock().now();
    let (key, viewer) = signer
        .verify(&key, &exp, &viewer, &sig, now)
        .map_err(|err| {
            tracing::info!(event = "media.url_refused", reason = %err);
            ApiError::Forbidden
        })?;
    let store = state.media.store().ok_or(ApiError::Unavailable {
        retry_after_secs: 30,
    })?;
    let bytes = store
        .get(Bucket::Clean, &key)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    tracing::debug!(event = "media.served", viewer_id = %viewer);
    let max_age = exp.parse::<i64>().unwrap_or(0) - now.unix_timestamp();
    let mut response = (StatusCode::OK, bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/webp"));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline; filename=\"card.webp\""),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("private, max-age={}", max_age.max(0))) {
        headers.insert(header::CACHE_CONTROL, value);
    }
    Ok(response)
}
