//! Push token registration (T-85, US-31, US-16, SR-15, SR-30). A session registers tokens only for
//! its own device: any other device id, including another account's, is the same 404 as a missing
//! one. Tokens are encrypted at rest, never echoed, never logged, and die with the session.

use axum::extract::{Path, State};
use http::StatusCode;
use notifications::{Provider, PushToken, Registrant, TokenKind};
use platform::problem::{ApiError, ProblemDetails};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::state::AppState;

pub const TAG: &str = "devices";

/// Body of `PUT /v1/devices/{deviceId}/push-tokens`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PushTokenRequest {
    /// `apns` (iOS) or `fcm` (Android).
    #[schema(example = "apns")]
    pub provider: String,
    /// `alert` (user-visible notifications) or `widget` (silent widget refreshes).
    #[schema(example = "alert")]
    pub kind: String,
    /// The provider token: APNs hex device token, or FCM registration token.
    pub token: String,
}

/// The caller's own device, or 404 (also for a malformed id and for sessions without a device).
fn own_device(caller: &Caller, raw: &str) -> Result<Uuid, ApiError> {
    let id = Uuid::try_parse(raw).map_err(|_| ApiError::NotFound)?;
    match caller.0.device_id {
        Some(device) if device == id => Ok(id),
        _ => Err(ApiError::NotFound),
    }
}

/// Register a push token for this device. Auth: ses.
///
/// Replaces the device's token of the same kind. A token already registered elsewhere (another
/// account or device) moves here; the old holder stops receiving. The token only works while the
/// session that registered it is live (logout, suspension or deletion stop pushes).
#[utoipa::path(
    put,
    path = "/v1/devices/{deviceId}/push-tokens",
    tag = TAG,
    operation_id = "putPushToken",
    security(("bearer" = [])),
    params(("deviceId" = String, Path, format = "uuid", description = "The calling session's device id.")),
    request_body = PushTokenRequest,
    responses(
        (status = NO_CONTENT, description = "Registered."),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: unknown provider or kind, or a malformed token.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: not the calling session's device.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn put_push_token(
    State(state): State<AppState>,
    caller: Caller,
    Path(device_id): Path<String>,
    body: ValidJson<PushTokenRequest>,
) -> Result<StatusCode, ApiError> {
    let device_id = own_device(&caller, &device_id)?;
    let body = body.0;
    let provider = Provider::parse(&body.provider)
        .ok_or_else(|| ApiError::Validation("provider must be apns or fcm".to_owned()))?;
    let kind = TokenKind::parse(&body.kind)
        .ok_or_else(|| ApiError::Validation("kind must be alert or widget".to_owned()))?;
    let token = PushToken::parse(provider, &body.token)
        .map_err(|_| ApiError::Validation("token is malformed for its provider".to_owned()))?;
    state
        .push_tokens
        .register(
            Registrant {
                account_id: caller.0.account_id,
                session_id: caller.0.session_id,
                device_id,
            },
            provider,
            kind,
            &token,
        )
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove this device's push tokens. Auth: onb/ses.
///
/// Idempotent: a device without tokens also answers 204.
#[utoipa::path(
    delete,
    path = "/v1/devices/{deviceId}/push-tokens",
    tag = TAG,
    operation_id = "deletePushTokens",
    security(("bearer" = [])),
    params(("deviceId" = String, Path, format = "uuid", description = "The calling session's device id.")),
    responses(
        (status = NO_CONTENT, description = "Removed."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: not the calling session's device.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn delete_push_tokens(
    State(state): State<AppState>,
    caller: Caller,
    Path(device_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let device_id = own_device(&caller, &device_id)?;
    state
        .push_tokens
        .remove_device(caller.0.account_id, device_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}
