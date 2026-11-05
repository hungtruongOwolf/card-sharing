//! The caller's own account (T-27): status, onboarding step and resolved policy flags.
//! Every query is keyed by the session's account id; no path or body parameter can name
//! another account.

use axum::Json;
use axum::extract::State;
use http::HeaderMap;
use identity::AccountStatus;
use platform::problem::{ApiError, OrNotFound, ProblemDetails};
use policy::{Policy, PolicyContext};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::Caller;
use crate::state::AppState;

pub const TAG: &str = "me";

#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub account: AccountView,
    pub onboarding: OnboardingView,
    /// Region rules that apply to this account (strictest of the signals stored at sign-up and
    /// the current request). The app renders only what these allow.
    pub policy: PolicyView,
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

#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyView {
    pub min_age: u8,
    pub ai_effects: AiEffectsView,
    pub ads: AdsView,
    pub ai_label: AiLabelView,
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
    let account = state
        .identity
        .account(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?
        .or_not_found()?;
    let current = PolicyContext {
        ip_country: state.ip_country(&headers),
        ..PolicyContext::default()
    };
    let policy = state
        .policy
        .resolve_strictest(&[account.region_signals.context(), current]);
    Ok(Json(MeResponse {
        account: AccountView {
            id: account.id,
            status: account.status,
        },
        onboarding: OnboardingView {
            step: onboarding_step(account.status),
        },
        policy: policy.into(),
    }))
}
