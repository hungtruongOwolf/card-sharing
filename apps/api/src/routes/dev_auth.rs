//! Development-only sign-in for the seed users (#306), so the app can be demoed against a local
//! server without Apple or Google accounts.
//!
//! - Served only when `API_DEV_SIGN_IN=true`, which config refuses unless `API_ENV` is
//!   development or test. Disabled, `/v1/dev/*` answers the same 404 as an unknown path.
//! - Only the fixed seed personas ([`crate::seed::PERSONAS`]) can be signed into: the handle maps
//!   to the provider subject `seed.<key>`, which no real Apple account can have. A real account
//!   is never reachable here, whatever its handle.
//! - The session is a normal one (same TTLs, refresh, revocation), audited as `auth.sign_in`
//!   with provider `dev`.

use axum::Json;
use axum::extract::State;
use identity::{AccountStatus, Provider, VerifiedIdentity};
use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::ValidJson;
use crate::routes::auth::{DeviceRequest, TokenPair};
use crate::seed::{PERSONAS, Persona, seed_subject};
use crate::state::AppState;

pub const TAG: &str = "dev";

/// One seed user the development sign-in can start a session for.
#[derive(Debug, Serialize, ToSchema)]
pub struct DevSeedUser {
    #[schema(example = "alice.demo")]
    pub handle: &'static str,
    #[schema(example = "Alice (demo)")]
    pub display_name: &'static str,
    /// Under 18: the app shows the minor experience for this account.
    pub minor: bool,
}

/// The seed users present in this database (`pnpm dev:seed` creates them).
#[derive(Debug, Serialize, ToSchema)]
pub struct DevSeedUsers {
    /// Empty until the seed has run.
    pub users: Vec<DevSeedUser>,
}

/// Body of `POST /v1/dev/sign-in`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DevSignInRequest {
    /// A seed user's handle from `GET /v1/dev/seed-users`.
    #[schema(min_length = 1, max_length = 32, example = "alice.demo")]
    pub handle: String,
    /// The signing-in device, as for provider sign-in.
    #[serde(default)]
    pub device: Option<DeviceRequest>,
}

/// List the seed users. Development and test servers only (`API_DEV_SIGN_IN=true`); elsewhere
/// this route does not exist (404). Auth: none.
#[utoipa::path(
    get,
    path = "/v1/dev/seed-users",
    tag = TAG,
    operation_id = "listDevSeedUsers",
    security(()),
    responses(
        (status = OK, description = "Seed users that exist in this database.", body = DevSeedUsers),
        (status = NOT_FOUND, description = "NOT_FOUND: the development sign-in is not enabled on this server.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_seed_users(
    State(state): State<AppState>,
) -> Result<Json<DevSeedUsers>, ApiError> {
    ensure_enabled(&state)?;
    let mut users = Vec::with_capacity(PERSONAS.len());
    for persona in PERSONAS {
        if seed_account(&state, persona).await?.is_some() {
            users.push(DevSeedUser {
                handle: persona.handle,
                display_name: persona.display_name,
                minor: persona.age < 18,
            });
        }
    }
    Ok(Json(DevSeedUsers { users }))
}

/// Sign in as a seed user and get a normal session. Development and test servers only
/// (`API_DEV_SIGN_IN=true`); elsewhere this route does not exist (404). Auth: none.
#[utoipa::path(
    post,
    path = "/v1/dev/sign-in",
    tag = TAG,
    operation_id = "devSignIn",
    security(()),
    request_body = DevSignInRequest,
    responses(
        (status = OK, description = "Signed in: the same token pair as provider sign-in.", body = TokenPair),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (unknown fields are rejected).", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED: the seed account is suspended or banned.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: not enabled on this server, not a seed handle, or the seed has not run.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn dev_sign_in(
    State(state): State<AppState>,
    body: ValidJson<DevSignInRequest>,
) -> Result<Json<TokenPair>, ApiError> {
    ensure_enabled(&state)?;
    let body = body.0;
    let persona = PERSONAS
        .into_iter()
        .find(|persona| persona.handle == body.handle)
        .ok_or(ApiError::NotFound)?;
    let device = body.device.map(DeviceRequest::into_info).transpose()?;
    let account_id = seed_account(&state, persona)
        .await?
        .ok_or(ApiError::NotFound)?;
    let account = state
        .identity
        .account(account_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    if matches!(
        account.status,
        AccountStatus::Suspended | AccountStatus::Banned
    ) {
        return Err(ApiError::AccountSuspended);
    }
    let session = state
        .identity
        .start_session(account_id, device.as_ref())
        .await
        .map_err(ApiError::internal)?;
    let entry = AuditEntry::new(Actor::User(account_id), AuditAction::SignIn)
        .target("session", session.session_id)
        .detail("provider", "dev");
    audit::record_or_log(state.identity.pool(), &entry, state.identity.clock().now()).await;
    tracing::warn!(
        event = "auth.sign_in",
        provider = "dev",
        account_id = %account_id,
        "development sign-in"
    );
    Ok(Json(session.into()))
}

/// Defense in depth: the router already hides `/v1/dev/*` when the flag is off.
fn ensure_enabled(state: &AppState) -> Result<(), ApiError> {
    if state.dev_sign_in {
        Ok(())
    } else {
        Err(ApiError::NotFound)
    }
}

/// The persona's account, found by its seed identity only (never by handle).
async fn seed_account(state: &AppState, persona: Persona) -> Result<Option<Uuid>, ApiError> {
    let identity = VerifiedIdentity {
        provider: Provider::Apple,
        subject: seed_subject(persona.key),
        email: None,
    };
    state
        .identity
        .find_account(&identity)
        .await
        .map_err(ApiError::internal)
}
