//! Sign-in and session endpoints (ADR-0004): Apple sign-in (T-19), refresh-token rotation and
//! logout (T-18).

use axum::Json;
use axum::extract::State;
use http::{HeaderMap, StatusCode};
use identity::{
    DeviceInfo, IssuedSession, Platform, Provider, RefreshError, RegionSignals, RevokeReason,
    SignInError, SignInRequest,
};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::state::AppState;

pub const TAG: &str = "auth";

/// Body of `POST /v1/auth/refresh`. Only the refresh token: the session (and therefore the
/// account) is derived from it, never from a client-supplied id.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    #[schema(min_length = 1, max_length = 256)]
    pub refresh_token: String,
}

/// A new access + refresh token pair. Store both in the device's secure storage only.
#[derive(Debug, Serialize, ToSchema)]
pub struct TokenPair {
    #[schema(example = "Bearer")]
    pub token_type: &'static str,
    /// Opaque, 10 minutes. Send as `Authorization: Bearer <access_token>`.
    pub access_token: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub access_expires_at: OffsetDateTime,
    /// Opaque, single use. Replaying a used one signs out the whole session.
    pub refresh_token: String,
}

impl From<IssuedSession> for TokenPair {
    fn from(session: IssuedSession) -> Self {
        Self {
            token_type: "Bearer",
            access_token: session.access_token.expose().to_owned(),
            access_expires_at: session.access_expires_at,
            refresh_token: session.refresh_token.expose().to_owned(),
        }
    }
}

/// Body of `POST /v1/auth/apple`. The account is derived from the verified token only.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AppleSignInRequest {
    /// Apple identity token (JWS) from the native Sign in with Apple sheet.
    #[schema(min_length = 1, max_length = 8192)]
    pub id_token: String,
    /// The raw nonce the app generated for this sign-in (16 to 256 visible ASCII characters,
    /// single use). The app passes `lowercase hex SHA-256(nonce)` to Apple; the token's `nonce`
    /// claim must match.
    #[schema(min_length = 16, max_length = 256)]
    pub nonce: String,
    /// One-time authorization code from the same sheet. The server exchanges it for an Apple
    /// refresh token (kept encrypted, revoked on account deletion). Optional.
    #[schema(min_length = 1, max_length = 1024)]
    #[serde(default)]
    pub authorization_code: Option<String>,
    /// The signing-in device. Recommended: device-scoped limits and cooldowns use it.
    #[serde(default)]
    pub device: Option<DeviceRequest>,
}

/// The device a session is issued to. `id` is a UUID the app generates once per install.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceRequest {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub platform: DevicePlatform,
    #[schema(min_length = 1, max_length = 32, example = "1.0.0")]
    pub app_version: String,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DevicePlatform {
    Ios,
    Android,
}

impl DeviceRequest {
    fn into_info(self) -> Result<DeviceInfo, ApiError> {
        if self.app_version.is_empty()
            || self.app_version.len() > 32
            || !self.app_version.chars().all(|c| c.is_ascii_graphic())
        {
            return Err(ApiError::Validation(
                "device.app_version must be 1 to 32 visible ASCII characters".to_owned(),
            ));
        }
        Ok(DeviceInfo {
            id: self.id,
            platform: match self.platform {
                DevicePlatform::Ios => Platform::Ios,
                DevicePlatform::Android => Platform::Android,
            },
            app_version: self.app_version,
        })
    }
}

/// Shared field checks for provider sign-in bodies.
fn check_token_and_nonce(id_token: &str, nonce: &str) -> Result<(), ApiError> {
    if id_token.is_empty() || id_token.len() > 8192 {
        return Err(ApiError::Validation(
            "id_token must be 1 to 8192 characters".to_owned(),
        ));
    }
    if nonce.len() < 16 || nonce.len() > 256 || !nonce.chars().all(|c| c.is_ascii_graphic()) {
        return Err(ApiError::Validation(
            "nonce must be 16 to 256 visible ASCII characters".to_owned(),
        ));
    }
    Ok(())
}

/// Sign in with Apple. Auth: none (the ID token is the credential). Creates the account on first
/// sign-in (status `pending_age`, onboarding session); never links accounts by email (AC-1.3).
#[utoipa::path(
    post,
    path = "/v1/auth/apple",
    tag = TAG,
    operation_id = "signInWithApple",
    security(()),
    request_body = AppleSignInRequest,
    responses(
        (status = OK, description = "Signed in: an onboarding session for a new or unfinished account, a full session otherwise. Call GET /v1/me for the onboarding step.", body = TokenPair),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (unknown fields are rejected).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: invalid, expired, forged or misaddressed ID token, nonce mismatch, or a replayed nonce.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED: the account may not sign in (suspended, banned or deletion pending).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: provider not configured or unreachable, or a fail-closed store is down.", body = ProblemDetails),
    )
)]
pub async fn sign_in_apple(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: ValidJson<AppleSignInRequest>,
) -> Result<Json<TokenPair>, ApiError> {
    let body = body.0;
    check_token_and_nonce(&body.id_token, &body.nonce)?;
    if body
        .authorization_code
        .as_ref()
        .is_some_and(|code| code.is_empty() || code.len() > 1024)
    {
        return Err(ApiError::Validation(
            "authorization_code must be 1 to 1024 characters".to_owned(),
        ));
    }
    let request = SignInRequest {
        id_token: body.id_token,
        raw_nonce: body.nonce,
        authorization_code: body.authorization_code,
        device: body.device.map(DeviceRequest::into_info).transpose()?,
        signals: sign_up_signals(&state, &headers),
    };
    sign_in(&state, Provider::Apple, request).await
}

/// Region signals recorded on a new account: the trusted edge's IP country (SR-24).
fn sign_up_signals(state: &AppState, headers: &HeaderMap) -> RegionSignals {
    RegionSignals {
        ip: state.ip_country(headers).map(|c| c.to_string()),
        ..RegionSignals::default()
    }
}

async fn sign_in(
    state: &AppState,
    provider: Provider,
    request: SignInRequest,
) -> Result<Json<TokenPair>, ApiError> {
    let service = state.sign_in.as_ref().ok_or(ApiError::Unavailable {
        retry_after_secs: 60,
    })?;
    match service.sign_in(provider, request).await {
        Ok(signed_in) => Ok(Json(signed_in.session.into())),
        Err(SignInError::Rejected(reason)) => {
            tracing::info!(provider = provider.as_str(), reason, "ID token rejected");
            Err(ApiError::Unauthenticated)
        }
        Err(SignInError::Replayed) => Err(ApiError::Unauthenticated),
        Err(SignInError::Blocked(status)) => {
            tracing::info!(provider = provider.as_str(), %status, "sign-in refused for a blocked account");
            Err(ApiError::AccountSuspended)
        }
        Err(SignInError::Unavailable(reason)) => {
            tracing::warn!(provider = provider.as_str(), reason, "sign-in unavailable");
            Err(ApiError::Unavailable {
                retry_after_secs: 30,
            })
        }
        Err(SignInError::Store(err)) => Err(ApiError::internal(err)),
    }
}

/// Rotate a refresh token. Auth: none (the refresh token in the body is the credential).
#[utoipa::path(
    post,
    path = "/v1/auth/refresh",
    tag = TAG,
    operation_id = "refreshSession",
    security(()),
    request_body = RefreshRequest,
    responses(
        (status = OK, description = "New token pair; the presented refresh token is now used up.", body = TokenPair),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (unknown fields are rejected).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED (unknown token) or SESSION_REVOKED (reused, revoked or ended session: sign in again).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE (rate-limit store down; auth fails closed).", body = ProblemDetails),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    body: ValidJson<RefreshRequest>,
) -> Result<Json<TokenPair>, ApiError> {
    let token = body.0.refresh_token;
    if token.is_empty() || token.len() > 256 {
        return Err(ApiError::Validation(
            "refresh_token must be 1 to 256 characters".to_owned(),
        ));
    }
    match state.identity.refresh(&token).await {
        Ok(session) => Ok(Json(session.into())),
        Err(RefreshError::Unknown) => Err(ApiError::Unauthenticated),
        Err(RefreshError::Revoked | RefreshError::Reused) => Err(ApiError::SessionRevoked),
        Err(RefreshError::Store(err)) => Err(ApiError::internal(err)),
    }
}

/// Log out this device: revokes the current session (and its widget token). Auth: onb/ses.
#[utoipa::path(
    delete,
    path = "/v1/sessions/current",
    tag = TAG,
    operation_id = "deleteCurrentSession",
    security(("bearer" = [])),
    responses(
        (status = NO_CONTENT, description = "Session revoked; its tokens stop working immediately."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
    )
)]
pub async fn delete_current_session(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<StatusCode, ApiError> {
    state
        .identity
        .revoke_session(caller.0.session_id, RevokeReason::Logout)
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}
