//! Sign-in and session endpoints (ADR-0004): Apple (T-19) and Google (T-20) sign-in, email magic
//! links (T-22 start, T-23 verify, ADR-0012), refresh-token rotation and logout (T-18).

use axum::Json;
use axum::extract::State;
use http::{HeaderMap, StatusCode};
use identity::{
    DeviceInfo, IssuedSession, LinkSignInError, MagicLinkError, Platform, Provider, RefreshError,
    RegionSignals, RevokeReason, SignInError, SignInRequest,
};
use platform::problem::{ApiError, ProblemDetails};
use platform::rate_limit::{AccountSubject, Decision, STORE_RETRY_AFTER_SECS, Subject};
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
    /// Sign-up invite code (ADR-0012), needed only when this sign-in creates an account while
    /// the sign-up gate is on; ignored otherwise. Letters, digits, `-` and spaces as typed.
    #[schema(min_length = 1, max_length = 32)]
    #[serde(default)]
    pub invite_code: Option<String>,
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
    pub(crate) fn into_info(self) -> Result<DeviceInfo, ApiError> {
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
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED: the account may not sign in (suspended, banned or deletion pending). INVITE_REQUIRED / INVITE_INVALID: this sign-in would create an account and the sign-up gate needs a valid invite code (the nonce is not used up: send the same token again with a code).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: provider not configured or unreachable, or the nonce store is down. SERVER_BUSY: the rate-limit store is slow or down (fails closed); retry after Retry-After.", body = ProblemDetails),
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
    check_invite_code(body.invite_code.as_deref())?;
    let request = SignInRequest {
        id_token: body.id_token,
        raw_nonce: body.nonce,
        authorization_code: body.authorization_code,
        device: body.device.map(DeviceRequest::into_info).transpose()?,
        signals: sign_up_signals(&state, &headers),
        signup: state.signup.gate(body.invite_code),
    };
    sign_in(&state, Provider::Apple, request).await
}

/// Body of `POST /v1/auth/google`. The account is derived from the verified token only.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GoogleSignInRequest {
    /// Google ID token (JWS) from the native Google sign-in.
    #[schema(min_length = 1, max_length = 8192)]
    pub id_token: String,
    /// The raw nonce the app generated for this sign-in (16 to 256 visible ASCII characters,
    /// single use). The app passes `lowercase hex SHA-256(nonce)` to Google as the sign-in nonce;
    /// the token's `nonce` claim must match.
    #[schema(min_length = 16, max_length = 256)]
    pub nonce: String,
    /// The signing-in device. Recommended: device-scoped limits and cooldowns use it.
    #[serde(default)]
    pub device: Option<DeviceRequest>,
    /// Sign-up invite code (ADR-0012), needed only when this sign-in creates an account while
    /// the sign-up gate is on; ignored otherwise. Letters, digits, `-` and spaces as typed.
    #[schema(min_length = 1, max_length = 32)]
    #[serde(default)]
    pub invite_code: Option<String>,
}

/// Sign in with Google. Auth: none (the ID token is the credential). Creates the account on first
/// sign-in (status `pending_age`, onboarding session); never links accounts by email (AC-1.3), so
/// the same email as an Apple account is a separate account.
#[utoipa::path(
    post,
    path = "/v1/auth/google",
    tag = TAG,
    operation_id = "signInWithGoogle",
    security(()),
    request_body = GoogleSignInRequest,
    responses(
        (status = OK, description = "Signed in: an onboarding session for a new or unfinished account, a full session otherwise. Call GET /v1/me for the onboarding step.", body = TokenPair),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (unknown fields are rejected).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: invalid, expired, forged or misaddressed ID token (including a token for another client id), nonce mismatch, or a replayed nonce.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED: the account may not sign in (suspended, banned or deletion pending). INVITE_REQUIRED / INVITE_INVALID: this sign-in would create an account and the sign-up gate needs a valid invite code (the nonce is not used up: send the same token again with a code).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: provider not configured or unreachable, or the nonce store is down. SERVER_BUSY: the rate-limit store is slow or down (fails closed); retry after Retry-After.", body = ProblemDetails),
    )
)]
pub async fn sign_in_google(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: ValidJson<GoogleSignInRequest>,
) -> Result<Json<TokenPair>, ApiError> {
    let body = body.0;
    check_token_and_nonce(&body.id_token, &body.nonce)?;
    check_invite_code(body.invite_code.as_deref())?;
    let request = SignInRequest {
        id_token: body.id_token,
        raw_nonce: body.nonce,
        authorization_code: None,
        device: body.device.map(DeviceRequest::into_info).transpose()?,
        signals: sign_up_signals(&state, &headers),
        signup: state.signup.gate(body.invite_code),
    };
    sign_in(&state, Provider::Google, request).await
}

/// Length check of an optional invite code; its content is judged by the gate (a malformed code
/// is just a wrong code, so the answer does not tell malformed from unknown).
fn check_invite_code(code: Option<&str>) -> Result<(), ApiError> {
    if code.is_some_and(|code| code.is_empty() || code.len() > identity::INVITE_CODE_MAX_INPUT) {
        return Err(ApiError::Validation(
            "invite_code must be 1 to 32 characters".to_owned(),
        ));
    }
    Ok(())
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
        Err(SignInError::InviteRequired) => Err(ApiError::InviteRequired),
        Err(SignInError::InviteInvalid) => Err(ApiError::InviteInvalid),
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

/// Body of `POST /v1/auth/email/start`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailStartRequest {
    #[schema(min_length = 3, max_length = 320, example = "cat@example.com")]
    pub email: String,
    /// PKCE S256 challenge: base64url (no padding) of SHA-256 of a random verifier the app keeps
    /// on this device (43 characters). Verifying the link (T-23) needs that verifier.
    #[schema(min_length = 43, max_length = 43)]
    pub code_challenge: String,
    /// Only `S256` is supported (default).
    #[serde(default)]
    pub code_challenge_method: Option<String>,
}

/// Answer of `POST /v1/auth/email/start`: the same shape for every well-formed request.
#[derive(Debug, Serialize, ToSchema)]
pub struct EmailStartResponse {
    /// Id of this request (a fresh UUID every time, whether or not the email has an account). The
    /// app keeps its PKCE verifier under it; the emailed link carries it back as
    /// `#rid=<request_id>&token=<token>`.
    #[schema(value_type = String, format = "uuid")]
    pub request_id: Uuid,
}

/// Request an email magic link. Auth: none. Always 202 for a well-formed request, whether or not
/// the email has an account (no enumeration, AC-1.2); the email is sent asynchronously. The link
/// works once, for 15 minutes, and only together with this device's PKCE verifier.
#[utoipa::path(
    post,
    path = "/v1/auth/email/start",
    tag = TAG,
    operation_id = "startEmailSignIn",
    security(()),
    request_body = EmailStartRequest,
    responses(
        (status = ACCEPTED, description = "If the address can receive email, a sign-in link is on its way.", body = EmailStartResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (malformed email or challenge, unknown fields).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED: more than 5 requests in 15 minutes from this IP or for this email (same body either way).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed; retry after Retry-After).", body = ProblemDetails),
    )
)]
pub async fn start_email_sign_in(
    State(state): State<AppState>,
    body: ValidJson<EmailStartRequest>,
) -> Result<(StatusCode, Json<EmailStartResponse>), ApiError> {
    let body = body.0;
    if body
        .code_challenge_method
        .as_deref()
        .is_some_and(|method| method != "S256")
    {
        return Err(ApiError::Validation(
            "code_challenge_method must be S256".to_owned(),
        ));
    }
    let email = identity::normalize_email(&body.email)
        .ok_or_else(|| ApiError::Validation("email is not a valid address".to_owned()))?;
    // Second window, keyed by the email (hashed) so rotating IPs cannot flood one inbox.
    let hash = identity::email_hash(&email);
    let mut subject_id = [0u8; 16];
    subject_id.copy_from_slice(&hash.as_bytes()[..16]);
    let subject = Subject {
        account: Some(AccountSubject {
            id: Uuid::from_bytes(subject_id),
            created_at: OffsetDateTime::UNIX_EPOCH,
        }),
        device: None,
        ip: None,
    };
    match state.limiter.check(EMAIL_LIMIT, &subject).await {
        Ok(Decision::Allowed) => {}
        Ok(Decision::Limited { retry_after_secs }) => {
            return Err(ApiError::RateLimited { retry_after_secs });
        }
        Ok(Decision::Unavailable) => {
            return Err(ApiError::ServerBusy {
                retry_after_secs: STORE_RETRY_AFTER_SECS,
            });
        }
        Err(err) => return Err(ApiError::internal(err)),
    }
    let request_id = state
        .identity
        .start_magic_link(&email, &body.code_challenge, &state.field_cipher)
        .await
        .map_err(|err| match err {
            MagicLinkError::InvalidEmail | MagicLinkError::InvalidChallenge => {
                ApiError::Validation("email or code_challenge is invalid".to_owned())
            }
            other => ApiError::internal(other),
        })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(EmailStartResponse { request_id }),
    ))
}

/// Per-email window of `POST /v1/auth/email/start` (config/limits.yaml).
pub const EMAIL_LIMIT: &str = "auth.email.start.email";

/// Body of `POST /v1/auth/email/verify`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmailVerifyRequest {
    /// The link's token, from its fragment (`#rid=...&token=<token>`): 43 base64url characters.
    #[schema(min_length = 43, max_length = 43)]
    pub token: String,
    /// The PKCE verifier this device kept when it asked for the link (RFC 7636: 43 to 128
    /// characters of `A-Z a-z 0-9 - . _ ~`). Its SHA-256 must equal the challenge sent to start.
    #[schema(min_length = 43, max_length = 128)]
    pub code_verifier: String,
    /// The signing-in device. Recommended on phones (device-scoped limits use it); the web sends
    /// none.
    #[serde(default)]
    pub device: Option<DeviceRequest>,
    /// Sign-up invite code (ADR-0012), needed only when the link would create an account while
    /// the sign-up gate is on; ignored otherwise. Letters, digits, `-` and spaces as typed.
    #[schema(min_length = 1, max_length = 32)]
    #[serde(default)]
    pub invite_code: Option<String>,
}

/// Sign in with an email magic link (T-23). Auth: none (the link token plus this device's PKCE
/// verifier are the credential). Called by the app's landing screen after the user confirms; the
/// link itself (a GET) never signs anyone in.
///
/// Creates the account on first sign-in (status `pending_age`, onboarding session) and is
/// otherwise identical to provider sign-in. The account is keyed by the email (hashed) only and is
/// never linked to an Apple or Google account with the same address (AC-1.3). An unknown token
/// and a verifier from another browser or device get the same 401; a wrong verifier does not use
/// the link up.
///
/// While the sign-up gate is on (`config/signup.yaml`), a link that would create an account needs
/// `invite_code`: missing → 403 `INVITE_REQUIRED`, wrong → 403 `INVITE_INVALID`, both leaving the
/// link usable for the retry; the fifth wrong code per link expires it.
#[utoipa::path(
    post,
    path = "/v1/auth/email/verify",
    tag = TAG,
    operation_id = "verifyEmailSignIn",
    security(()),
    request_body = EmailVerifyRequest,
    responses(
        (status = OK, description = "Signed in: an onboarding session for a new or unfinished account, a full session otherwise. Call GET /v1/me for the onboarding step.", body = TokenPair),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (malformed token or verifier, unknown fields).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "MAGIC_LINK_INVALID: unknown link, or the verifier is not the one of the device that asked for it.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED: the account may not sign in (suspended or banned). INVITE_REQUIRED / INVITE_INVALID: the link would create an account and the sign-up gate needs a valid invite code (the link stays usable).", body = ProblemDetails),
        (status = GONE, description = "MAGIC_LINK_EXPIRED (older than 15 minutes) or MAGIC_LINK_USED (single use): ask for a new link.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed).", body = ProblemDetails),
    )
)]
pub async fn verify_email_sign_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: ValidJson<EmailVerifyRequest>,
) -> Result<Json<TokenPair>, ApiError> {
    let body = body.0;
    if !identity::valid_link_token(&body.token) {
        return Err(ApiError::Validation(
            "token must be 43 base64url characters".to_owned(),
        ));
    }
    if !identity::valid_code_verifier(&body.code_verifier) {
        return Err(ApiError::Validation(
            "code_verifier must be 43 to 128 characters of A-Z a-z 0-9 - . _ ~".to_owned(),
        ));
    }
    check_invite_code(body.invite_code.as_deref())?;
    let device = body.device.map(DeviceRequest::into_info).transpose()?;
    let signals = sign_up_signals(&state, &headers);
    let gate = state.signup.gate(body.invite_code);
    let signed_in = state
        .identity
        .sign_in_with_magic_link(
            &body.token,
            &body.code_verifier,
            device.as_ref(),
            &signals,
            &gate,
        )
        .await
        .map_err(|err| match err {
            LinkSignInError::Invalid => ApiError::MagicLinkInvalid,
            LinkSignInError::Expired => ApiError::MagicLinkExpired,
            LinkSignInError::Used => ApiError::MagicLinkUsed,
            LinkSignInError::Blocked(_) => ApiError::AccountSuspended,
            LinkSignInError::InviteRequired => ApiError::InviteRequired,
            LinkSignInError::InviteInvalid => ApiError::InviteInvalid,
            LinkSignInError::Store(err) => ApiError::internal(err),
        })?;
    Ok(Json(signed_in.session.into()))
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
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (per device; while the rate-limit store is down, per instance).", body = ProblemDetails),
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

/// Provider of a re-auth proof.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReauthProvider {
    Apple,
    Google,
}

/// Body of `POST /v1/auth/reauth`: a fresh provider proof for the signed-in account.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReauthRequest {
    pub provider: ReauthProvider,
    /// A fresh ID token from the provider's native sheet.
    #[schema(min_length = 1, max_length = 8192)]
    pub id_token: String,
    /// The raw nonce of this proof (16 to 256 visible ASCII characters, single use), as for
    /// sign-in.
    #[schema(min_length = 16, max_length = 256)]
    pub nonce: String,
}

/// A re-auth token: send it as `X-Reauth` (next to the normal bearer token) to `re` routes.
#[derive(Debug, Serialize, ToSchema)]
pub struct ReauthResponse {
    /// Opaque, single device session, 5 minutes. Never a bearer token.
    pub reauth_token: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
}

/// Mint a 5-minute re-auth token from a fresh provider proof. Auth: ses.
///
/// The proof must belong to the caller's own account: a valid proof of another account, an
/// unknown identity, a forged, expired or replayed token are all 401 `UNAUTHENTICATED`. The token
/// is bound to the calling device session and replaces any earlier one of it.
#[utoipa::path(
    post,
    path = "/v1/auth/reauth",
    tag = TAG,
    operation_id = "reauthenticate",
    security(("bearer" = [])),
    request_body = ReauthRequest,
    responses(
        (status = OK, description = "Re-auth token for this device session (5 minutes).", body = ReauthResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (fields, unknown fields).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED (bad proof, another account's proof, replayed nonce), SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (5 per 15 minutes).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE (provider not configured or its keys unreachable).", body = ProblemDetails),
    )
)]
pub async fn reauthenticate(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<ReauthRequest>,
) -> Result<Json<ReauthResponse>, ApiError> {
    let body = body.0;
    check_token_and_nonce(&body.id_token, &body.nonce)?;
    let sign_in = state.sign_in.as_ref().ok_or(ApiError::Unavailable {
        retry_after_secs: 30,
    })?;
    let provider = match body.provider {
        ReauthProvider::Apple => Provider::Apple,
        ReauthProvider::Google => Provider::Google,
    };
    let account = match sign_in
        .verify_proof(provider, &body.id_token, &body.nonce)
        .await
    {
        Ok(account) => account,
        Err(SignInError::Unavailable(reason)) => {
            tracing::warn!(provider = provider.as_str(), reason, "re-auth unavailable");
            return Err(ApiError::Unavailable {
                retry_after_secs: 30,
            });
        }
        Err(SignInError::Store(err)) => return Err(ApiError::internal(err)),
        Err(err) => {
            tracing::info!(provider = provider.as_str(), error = %err, "re-auth proof refused");
            return Err(ApiError::Unauthenticated);
        }
    };
    if account != Some(caller.0.account_id) {
        tracing::info!(
            provider = provider.as_str(),
            "re-auth proof of another identity refused"
        );
        return Err(ApiError::Unauthenticated);
    }
    let token = state
        .identity
        .issue_reauth_token(caller.0.account_id, caller.0.session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::Unauthenticated)?;
    Ok(Json(ReauthResponse {
        reauth_token: token.token.expose().to_owned(),
        expires_at: token.expires_at,
    }))
}
