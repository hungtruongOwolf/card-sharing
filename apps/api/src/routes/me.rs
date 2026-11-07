//! The caller's own account (T-27): status, onboarding step, profile (T-31: display name, T-32:
//! handle) and resolved policy flags.
//! Every query is keyed by the session's account id; no path or body parameter can name
//! another account.

use axum::Json;
use axum::extract::State;
use http::HeaderMap;
use identity::{
    AccountStatus, AgeGateError, AgeOutcome, AgeSubmission, HandleError, Profile, ProfileError,
    ProfileUpdate,
};
use platform::problem::{ApiError, OrNotFound, ProblemDetails};
use platform::text::TextError;
use policy::{CountryCode, Policy, PolicyContext};
use serde::{Deserialize, Serialize};
use time::Date;
use time::format_description::well_known::Iso8601;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::state::AppState;

pub const TAG: &str = "me";

#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub account: AccountView,
    pub onboarding: OnboardingView,
    /// What the caller has set so far (empty fields before the profile step).
    pub profile: ProfileView,
    /// Region rules that apply to this account (strictest of the signals stored at sign-up and
    /// the current request). The app renders only what these allow.
    pub policy: PolicyView,
    pub settings: SettingsView,
}

/// The caller's privacy settings, as they apply right now.
#[derive(Debug, Serialize, ToSchema)]
pub struct SettingsView {
    /// Whether others can find me by exact handle. Always `false` under the region's discovery
    /// age (the toggle cannot be turned on then).
    pub findable_by_handle: bool,
    /// Whether this account may turn `findable_by_handle` on (false under the discovery age).
    pub can_be_findable: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountView {
    #[schema(value_type = String, example = "0192f0c1-7c1e-7a3b-9d2e-123456789abc")]
    pub id: Uuid,
    #[schema(value_type = String, example = "pending_age")]
    pub status: AccountStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStep {
    /// Date of birth not submitted yet.
    Age,
    /// Display name and handle not set yet.
    Profile,
    /// Onboarding finished.
    Done,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OnboardingView {
    pub step: OnboardingStep,
}

/// The caller's own profile. Display names are stored sanitized (what is returned is what
/// others will see).
#[derive(Debug, Serialize, ToSchema)]
pub struct ProfileView {
    #[schema(example = "Mai")]
    pub display_name: Option<String>,
    /// Unique, lowercase.
    #[schema(example = "mai.cat")]
    pub handle: Option<String>,
}

impl From<Profile> for ProfileView {
    fn from(profile: Profile) -> Self {
        Self {
            display_name: profile.display_name,
            handle: profile.handle,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyView {
    pub min_age: u8,
    pub ai_effects: AiEffectsView,
    pub ads: AdsView,
    pub ai_label: AiLabelView,
    /// Whether the caller may create public board links (#321). Client hint only: the create
    /// route stays the authority.
    pub public_links: PublicLinksView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicLinksView {
    /// `false` under the region's `public_links.min_age` (default 18), resolved with the same
    /// strictest-signal rule as `POST /v1/boards/{id}/public-links`, and for accounts that have
    /// not passed the age gate. The app hides "Share link" when it is `false` (AC-24.6).
    pub allowed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AiEffectsView {
    pub cloud_allowed: bool,
    /// `any` | `on_device_or_non_gemini` | `on_device_only`.
    #[schema(example = "on_device_or_non_gemini")]
    pub people_in_photo: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdsView {
    pub rewarded: bool,
    pub personalized_requires_consent: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AiLabelView {
    pub visible_badge: bool,
    pub c2pa: bool,
}

impl From<Policy> for PolicyView {
    fn from(policy: Policy) -> Self {
        let people_in_photo = serde_json::to_value(policy.ai_effects.people_in_photo)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        Self {
            min_age: policy.min_age,
            ai_effects: AiEffectsView {
                cloud_allowed: policy.ai_effects.cloud_allowed,
                people_in_photo,
            },
            ads: AdsView {
                rewarded: policy.ads.rewarded,
                personalized_requires_consent: policy.ads.personalized_requires_consent,
            },
            ai_label: AiLabelView {
                visible_badge: policy.ai_label.visible_badge,
                c2pa: policy.ai_label.c2pa,
            },
            // Age-dependent: filled in by `get_me`; closed until then.
            public_links: PublicLinksView { allowed: false },
        }
    }
}

pub fn onboarding_step(status: AccountStatus) -> OnboardingStep {
    match status {
        AccountStatus::PendingAge => OnboardingStep::Age,
        AccountStatus::PendingProfile => OnboardingStep::Profile,
        _ => OnboardingStep::Done,
    }
}

/// My account. Auth: onb/ses.
#[utoipa::path(
    get,
    path = "/v1/me",
    tag = TAG,
    operation_id = "getMe",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The caller's own account.", body = MeResponse),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = UPGRADE_REQUIRED, description = "UPGRADE_REQUIRED (X-App-Version below the minimum).", body = ProblemDetails),
    )
)]
pub async fn get_me(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, ApiError> {
    let id = caller.0.account_id;
    let current = PolicyContext {
        ip_country: state.ip_country(&headers),
        ..PolicyContext::default()
    };
    // Three independent reads keyed by the caller's id: concurrent, not one after another.
    // Results are checked in the old order, so a missing account is still a 404.
    let (account, profile, age) = tokio::join!(
        state.identity.account(id),
        state.identity.profile(id),
        state.age.status(id, &current),
    );
    let account = account.map_err(ApiError::internal)?.or_not_found()?;
    let profile = profile.map_err(ApiError::internal)?;
    let age = age.map_err(ApiError::internal)?;
    let settings = settings_for(&state, account.id, &age).await?;
    let mut policy: PolicyView = state
        .policy
        .resolve_strictest(&[account.region_signals.context(), current])
        .into();
    policy.public_links.allowed = age.public_links;
    Ok(Json(MeResponse {
        account: AccountView {
            id: account.id,
            status: account.status,
        },
        onboarding: OnboardingView {
            step: onboarding_step(account.status),
        },
        profile: profile.into(),
        policy,
        settings,
    }))
}

async fn settings_view(
    state: &AppState,
    account_id: Uuid,
    current: &PolicyContext,
) -> Result<SettingsView, ApiError> {
    let age = state
        .age
        .status(account_id, current)
        .await
        .map_err(ApiError::internal)?;
    settings_for(state, account_id, &age).await
}

async fn settings_for(
    state: &AppState,
    account_id: Uuid,
    age: &identity::AgeStatus,
) -> Result<SettingsView, ApiError> {
    let can_be_findable = age.discoverable;
    let stored = state
        .identity
        .findable_by_handle(account_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(SettingsView {
        findable_by_handle: stored && can_be_findable,
        can_be_findable,
    })
}

/// Body of `PATCH /v1/me/settings`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsRequest {
    /// Let others find me by exact handle (adults only).
    pub findable_by_handle: bool,
}

/// Change my privacy settings. Auth: ses.
///
/// Turning "Findable by handle" on needs the region's discovery age (`discovery.min_age`): below
/// it the answer is 403 `POLICY_DISALLOWED`. Turning it off is always allowed and takes effect on
/// the next search (the standard "no one found").
#[utoipa::path(
    patch,
    path = "/v1/me/settings",
    tag = TAG,
    operation_id = "updateSettings",
    security(("bearer" = [])),
    request_body = SettingsRequest,
    responses(
        (status = OK, description = "The settings now in force.", body = SettingsView),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "POLICY_DISALLOWED (under the discovery age), ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn update_settings(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    body: ValidJson<SettingsRequest>,
) -> Result<Json<SettingsView>, ApiError> {
    let account_id = caller.0.account_id;
    let current = PolicyContext {
        ip_country: state.ip_country(&headers),
        ..PolicyContext::default()
    };
    let findable = body.0.findable_by_handle;
    if findable {
        let status = state
            .age
            .status(account_id, &current)
            .await
            .map_err(ApiError::internal)?;
        if !status.discoverable {
            return Err(ApiError::PolicyDisallowed);
        }
    }
    state
        .identity
        .set_findable_by_handle(account_id, findable)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(settings_view(&state, account_id, &current).await?))
}

/// Body of `POST /v1/me/age`. Country hints can only make the policy stricter.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgeRequest {
    /// ISO 8601 calendar date, e.g. `2010-05-01`.
    #[schema(value_type = String, format = Date, example = "2010-05-01")]
    pub birth_date: String,
    /// Country the user says they live in (ISO 3166-1 alpha-2), if asked.
    #[schema(example = "US")]
    pub declared_country: Option<String>,
    /// App store storefront country reported by the OS (a hint, never trusted to loosen rules).
    #[schema(example = "US")]
    pub store_country: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AgeResponse {
    #[schema(value_type = String, example = "pending_profile")]
    pub status: AccountStatus,
    pub onboarding: OnboardingView,
}

fn parse_country(field: &str, value: Option<&str>) -> Result<Option<CountryCode>, ApiError> {
    value
        .map(|raw| {
            raw.parse().map_err(|_| {
                ApiError::Validation(format!("{field} must be an ISO 3166-1 alpha-2 code"))
            })
        })
        .transpose()
}

/// Submit the date of birth once. Auth: onb (account in `pending_age` only).
///
/// Refusal is neutral: 403 `AGE_GATE_REFUSED` with no threshold or region in the body; the
/// pending account is deleted and the same device / sign-in identity is refused for 24 h.
#[utoipa::path(
    post,
    path = "/v1/me/age",
    tag = TAG,
    operation_id = "submitAge",
    security(("bearer" = [])),
    request_body = AgeRequest,
    responses(
        (status = OK, description = "Admitted; continue with the profile step.", body = AgeResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "AGE_GATE_REFUSED (neutral; account removed), AGE_ALREADY_SUBMITTED (DOB is locked) or FORBIDDEN (not an onboarding session).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed) or TEMPORARILY_UNAVAILABLE (cooldown store down).", body = ProblemDetails),
    )
)]
pub async fn submit_age(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    body: ValidJson<AgeRequest>,
) -> Result<Json<AgeResponse>, ApiError> {
    let request = body.0;
    let birth_date = Date::parse(&request.birth_date, &Iso8601::DATE)
        .map_err(|_| ApiError::Validation("birth_date must be YYYY-MM-DD".to_owned()))?;
    let submission = AgeSubmission {
        birth_date,
        store_country: parse_country("store_country", request.store_country.as_deref())?,
        declared_country: parse_country("declared_country", request.declared_country.as_deref())?,
        ip_country: state.ip_country(&headers),
    };
    let outcome = state
        .identity
        .submit_age(
            caller.0.account_id,
            caller.0.device_id,
            &submission,
            &state.policy,
            &state.field_cipher,
        )
        .await
        .map_err(|err| match err {
            AgeGateError::AlreadySubmitted => ApiError::AgeAlreadySubmitted,
            AgeGateError::InvalidBirthDate => {
                ApiError::Validation("birth_date is not a plausible date of birth".to_owned())
            }
            AgeGateError::CooldownUnavailable(reason) => {
                tracing::error!(%reason, "age gate cooldown store unavailable");
                ApiError::Unavailable {
                    retry_after_secs: 5,
                }
            }
            other => ApiError::internal(other),
        })?;
    match outcome {
        AgeOutcome::Refused => Err(ApiError::AgeGateRefused),
        AgeOutcome::Admitted { status } => Ok(Json(AgeResponse {
            status,
            onboarding: OnboardingView {
                step: onboarding_step(status),
            },
        })),
    }
}

/// Body of `PUT /v1/me/profile`. The account is always the caller's; no id is accepted.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileRequest {
    /// 2-30 characters (grapheme clusters) after sanitizing; not unique, never searchable.
    #[schema(example = "Mai")]
    pub display_name: String,
}

/// Result of a profile write: the account status and onboarding step after it, and the profile.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProfileResponse {
    #[schema(value_type = String, example = "pending_profile")]
    pub status: AccountStatus,
    pub onboarding: OnboardingView,
    pub profile: ProfileView,
}

impl From<ProfileUpdate> for ProfileResponse {
    fn from(update: ProfileUpdate) -> Self {
        Self {
            status: update.status,
            onboarding: OnboardingView {
                step: onboarding_step(update.status),
            },
            profile: update.profile.into(),
        }
    }
}

fn text_problem(field: &str, err: &TextError) -> ApiError {
    if let TextError::Blocked { list } = err {
        // The list name is for metrics only; the client learns just "not allowed".
        tracing::info!(event = "text_filter.blocked", field, list = %list, "text refused");
    }
    ApiError::Validation(format!("{field} {err}"))
}

pub(crate) fn profile_error(err: ProfileError) -> ApiError {
    match err {
        ProfileError::NotReady => ApiError::OnboardingIncomplete,
        ProfileError::InvalidDisplayName(text) => text_problem("display_name", &text),
        ProfileError::InvalidHandle(handle) => {
            if let HandleError::Blocked { list } = &handle {
                tracing::info!(event = "text_filter.blocked", field = "handle", list = %list, "text refused");
            }
            ApiError::Validation(format!("handle {handle}"))
        }
        ProfileError::HandleTaken => ApiError::HandleTaken,
        ProfileError::HandleCooldown { retry_after_secs } => {
            ApiError::HandleChangeCooldown { retry_after_secs }
        }
        ProfileError::Store(store) => ApiError::internal(store),
    }
}

/// Set my display name. Auth: onb/ses (after the age gate).
///
/// The name is sanitized (NFC, invisible and bidi characters removed, whitespace collapsed,
/// combining marks capped) and checked against the term lists; the stored form is returned.
#[utoipa::path(
    put,
    path = "/v1/me/profile",
    tag = TAG,
    operation_id = "updateProfile",
    security(("bearer" = [])),
    request_body = ProfileRequest,
    responses(
        (status = OK, description = "Saved; the response carries the stored (sanitized) name.", body = ProfileResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (length, filtered term, unknown field).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE (age gate not passed) or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn update_profile(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<ProfileRequest>,
) -> Result<Json<ProfileResponse>, ApiError> {
    let update = state
        .identity
        .set_display_name(
            caller.0.account_id,
            &body.0.display_name,
            &state.profile.text_filter,
            &state.profile.rules,
        )
        .await
        .map_err(profile_error)?;
    Ok(Json(update.into()))
}

/// Body of `POST /v1/me/handle/check` and `PUT /v1/me/handle`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HandleRequest {
    /// `^[a-z0-9._]{3,20}$` after lowercasing (uppercase input is accepted and lowercased).
    #[schema(example = "mai.cat")]
    pub handle: String,
}

/// Availability of a handle for the caller. Nothing about any other account is returned.
#[derive(Debug, Serialize, ToSchema)]
pub struct HandleCheckResponse {
    /// `false` when taken, reserved or otherwise not claimable; `true` for the caller's own.
    pub available: bool,
}

/// Is this handle available to me? Auth: onb/ses (after the age gate).
///
/// Rate-limited (20/min and 100/day per user, fails closed): it necessarily reveals whether a
/// handle is taken, and nothing else.
#[utoipa::path(
    post,
    path = "/v1/me/handle/check",
    tag = TAG,
    operation_id = "checkHandle",
    security(("bearer" = [])),
    request_body = HandleRequest,
    responses(
        (status = OK, description = "Availability only.", body = HandleCheckResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (format or filtered term).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE (age gate not passed) or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed; retry after Retry-After).", body = ProblemDetails),
    )
)]
pub async fn check_handle(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<HandleRequest>,
) -> Result<Json<HandleCheckResponse>, ApiError> {
    let available = state
        .identity
        .handle_available(
            caller.0.account_id,
            &body.0.handle,
            &state.profile.text_filter,
            &state.profile.rules,
        )
        .await
        .map_err(profile_error)?;
    Ok(Json(HandleCheckResponse { available }))
}

/// Claim or change my handle. Auth: onb/ses (after the age gate).
///
/// Free to change for 24 h after the first claim; afterwards once per 30 days (both config). A
/// handle released after the grace window stays reserved for 30 days. Finishing the profile
/// (display name and handle set) makes the account `active` and the current session a full one.
#[utoipa::path(
    put,
    path = "/v1/me/handle",
    tag = TAG,
    operation_id = "claimHandle",
    security(("bearer" = [])),
    request_body = HandleRequest,
    responses(
        (status = OK, description = "Claimed (or already mine).", body = ProfileResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (format or filtered term).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE (age gate not passed) or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = CONFLICT, description = "HANDLE_TAKEN (taken or reserved; no detail).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "HANDLE_CHANGE_COOLDOWN (with Retry-After) or RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed; retry after Retry-After).", body = ProblemDetails),
    )
)]
pub async fn claim_handle(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<HandleRequest>,
) -> Result<Json<ProfileResponse>, ApiError> {
    let update = state
        .identity
        .claim_handle(
            caller.0.account_id,
            &body.0.handle,
            &state.profile.text_filter,
            &state.profile.rules,
        )
        .await
        .map_err(profile_error)?;
    Ok(Json(update.into()))
}
