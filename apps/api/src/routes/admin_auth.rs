//! Staff sign-in with MFA (T-144, US-27, SR-32). Server part only; the admin console UI is
//! deferred.
//!
//! 1. `POST /v1/admin/auth/login` with a fresh Apple/Google proof of a staff account → a 5-minute
//!    login token.
//! 2. `POST /v1/admin/auth/mfa` with that token and a TOTP code → the staff session cookie
//!    `__Host-staff_session` (HttpOnly, Secure, SameSite=Strict, Path=/) and a CSRF token that
//!    every state-changing staff request repeats in `X-CSRF-Token`.
//!
//! Every failure of either step is the same 401, so the routes do not reveal who is staff.

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use identity::{Provider, SignInError, StaffError};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{STAFF_COOKIE, StaffCaller, ValidJson};
use crate::routes::auth::ReauthProvider;
use crate::state::AppState;

pub const TAG: &str = "admin";

/// Body of `POST /v1/admin/auth/login`: a fresh provider proof, as for re-auth.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StaffLoginRequest {
    pub provider: ReauthProvider,
    #[schema(min_length = 1, max_length = 8192)]
    pub id_token: String,
    #[schema(min_length = 16, max_length = 256)]
    pub nonce: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StaffLoginChallenge {
    /// Opaque, single use, 5 minutes: send it with the TOTP code to `/v1/admin/auth/mfa`.
    pub mfa_token: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
}

/// Body of `POST /v1/admin/auth/mfa`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StaffMfaRequest {
    #[schema(min_length = 1, max_length = 256)]
    pub mfa_token: String,
    /// The 6-digit code from the authenticator app.
    #[schema(min_length = 6, max_length = 6, example = "123456")]
    pub code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StaffSessionView {
    /// Repeat in `X-CSRF-Token` on every state-changing staff request. Keep it in memory only.
    pub csrf_token: String,
    /// `moderator` or `admin`.
    #[schema(example = "moderator")]
    pub role: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub idle_expires_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub absolute_expires_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StaffMeView {
    #[schema(value_type = String, format = "uuid")]
    pub account_id: Uuid,
    #[schema(example = "moderator")]
    pub role: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub idle_expires_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub absolute_expires_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StaffMemberView {
    #[schema(value_type = String, format = "uuid")]
    pub account_id: Uuid,
    #[schema(example = "moderator")]
    pub role: &'static str,
    pub disabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StaffMemberList {
    pub items: Vec<StaffMemberView>,
}

fn refused(err: StaffError) -> ApiError {
    match err {
        StaffError::Refused => ApiError::Unauthenticated,
        StaffError::Database(err) => ApiError::internal(err),
        StaffError::Crypto(reason) => ApiError::internal(reason),
    }
}

/// Step one of a staff sign-in. Auth: none. Any failure (bad proof, not staff, disabled,
/// inactive account) is the same 401.
#[utoipa::path(
    post,
    path = "/v1/admin/auth/login",
    tag = TAG,
    operation_id = "staffLogin",
    security(()),
    request_body = StaffLoginRequest,
    responses(
        (status = OK, description = "Proof accepted: enter the TOTP code next.", body = StaffLoginChallenge),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED (one answer for every refusal).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (10 per 15 minutes per IP).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE (provider not configured or unreachable).", body = ProblemDetails),
    )
)]
pub async fn staff_login(
    State(state): State<AppState>,
    body: ValidJson<StaffLoginRequest>,
) -> Result<Json<StaffLoginChallenge>, ApiError> {
    let body = body.0;
    if body.id_token.is_empty()
        || body.id_token.len() > 8192
        || body.nonce.len() < 16
        || body.nonce.len() > 256
    {
        return Err(ApiError::Validation(
            "id_token or nonce has an invalid length".to_owned(),
        ));
    }
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
        Ok(Some(account)) => account,
        Ok(None) => return Err(ApiError::Unauthenticated),
        Err(SignInError::Unavailable(reason)) => {
            tracing::warn!(reason, "staff login: provider unavailable");
            return Err(ApiError::Unavailable {
                retry_after_secs: 30,
            });
        }
        Err(SignInError::Store(err)) => return Err(ApiError::internal(err)),
        Err(_) => return Err(ApiError::Unauthenticated),
    };
    let challenge = state.staff.start_login(account).await.map_err(refused)?;
    tracing::info!(event = "staff.login_started", account_id = %account);
    Ok(Json(StaffLoginChallenge {
        mfa_token: challenge.token.expose().to_owned(),
        expires_at: challenge.expires_at,
    }))
}

/// Step two: the TOTP code. Auth: none (the login token is the credential). Sets the staff
/// session cookie.
#[utoipa::path(
    post,
    path = "/v1/admin/auth/mfa",
    tag = TAG,
    operation_id = "staffMfa",
    security(()),
    request_body = StaffMfaRequest,
    responses(
        (status = OK, description = "Signed in: `Set-Cookie: __Host-staff_session=...; HttpOnly; Secure; SameSite=Strict; Path=/`.", body = StaffSessionView),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: unknown, expired or used login token, wrong or replayed code, too many attempts.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn staff_mfa(
    State(state): State<AppState>,
    body: ValidJson<StaffMfaRequest>,
) -> Result<Response, ApiError> {
    let body = body.0;
    if body.mfa_token.is_empty() || body.mfa_token.len() > 256 || body.code.len() > 16 {
        return Err(ApiError::Validation(
            "mfa_token or code has an invalid length".to_owned(),
        ));
    }
    let issued = state
        .staff
        .complete_login(&body.mfa_token, &body.code, &state.field_cipher)
        .await
        .map_err(refused)?;
    let cookie = format!(
        "{STAFF_COOKIE}={}; Path=/; Max-Age={}; HttpOnly; Secure; SameSite=Strict",
        issued.token.expose(),
        identity::STAFF_ABSOLUTE.whole_seconds()
    );
    let mut response = Json(StaffSessionView {
        csrf_token: issued.csrf_token.expose().to_owned(),
        role: issued.role.as_str(),
        idle_expires_at: issued.idle_expires_at,
        absolute_expires_at: issued.absolute_expires_at,
    })
    .into_response();
    let value = HeaderValue::from_str(&cookie).map_err(ApiError::internal)?;
    response.headers_mut().insert(header::SET_COOKIE, value);
    tracing::info!(event = "staff.signed_in", session_id = %issued.session_id);
    Ok(response)
}

/// End this staff session. Auth: staff (moderator or admin) + `X-CSRF-Token`.
#[utoipa::path(
    post,
    path = "/v1/admin/auth/logout",
    tag = TAG,
    operation_id = "staffLogout",
    security(("staff" = [])),
    responses(
        (status = NO_CONTENT, description = "Signed out; the cookie is cleared."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: no live staff session.", body = ProblemDetails),
        (status = FORBIDDEN, description = "FORBIDDEN: missing or wrong X-CSRF-Token, or IP not allowed.", body = ProblemDetails),
    )
)]
pub async fn staff_logout(
    State(state): State<AppState>,
    caller: StaffCaller,
) -> Result<Response, ApiError> {
    state
        .staff
        .logout(caller.0.session_id)
        .await
        .map_err(refused)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "__Host-staff_session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Strict",
        ),
    );
    Ok(response)
}

/// The signed-in staff member. Auth: staff (moderator or admin).
#[utoipa::path(
    get,
    path = "/v1/admin/me",
    tag = TAG,
    operation_id = "getStaffMe",
    security(("staff" = [])),
    responses(
        (status = OK, description = "The caller's staff role and session expiry.", body = StaffMeView),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: no live staff session (app bearer tokens never count).", body = ProblemDetails),
        (status = FORBIDDEN, description = "FORBIDDEN: IP not on the staff allow-list.", body = ProblemDetails),
    )
)]
pub async fn get_staff_me(caller: StaffCaller) -> Json<StaffMeView> {
    let principal = caller.0;
    Json(StaffMeView {
        account_id: principal.account_id,
        role: principal.role.as_str(),
        idle_expires_at: principal.idle_expires_at,
        absolute_expires_at: principal.absolute_expires_at,
    })
}

/// Every staff member. Auth: staff, **admin** only.
#[utoipa::path(
    get,
    path = "/v1/admin/staff",
    tag = TAG,
    operation_id = "listStaff",
    security(("staff" = [])),
    responses(
        (status = OK, description = "Staff members, newest first.", body = StaffMemberList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: no live staff session.", body = ProblemDetails),
        (status = FORBIDDEN, description = "FORBIDDEN: not an admin, or IP not allowed.", body = ProblemDetails),
    )
)]
pub async fn list_staff(
    State(state): State<AppState>,
    _caller: StaffCaller,
) -> Result<Json<StaffMemberList>, ApiError> {
    let members = state.staff.members().await.map_err(refused)?;
    Ok(Json(StaffMemberList {
        items: members
            .into_iter()
            .map(|member| StaffMemberView {
                account_id: member.account_id,
                role: member.role.as_str(),
                disabled: member.disabled,
                created_at: member.created_at,
            })
            .collect(),
    }))
}
