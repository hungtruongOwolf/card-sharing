//! Tier 2 cloud AI effects (sprint 9, genai-effects.md, ADR-0003) on a fake provider.
//!
//! - Who: `ses` only. Region and age come from the region policy (`ai_effects.cloud_allowed`,
//!   `ai_effects.min_age`, 18 by default for the Gemini 18+ terms): anyone else is refused with
//!   403 `POLICY_DISALLOWED`, and the overview tells the app to hide the feature.
//! - Consent: a photo goes to the (third-party) provider only after the caller agreed to the
//!   current consent version (`PUT /v1/ai-effects/consent`); otherwise 403 `CONSENT_REQUIRED`.
//! - Credits live only on the server: submit reserves the template's cost in the same
//!   transaction that queues the job; a refusal or failure refunds it.
//! - Curated templates only (`config/ai_effects.yaml`); no prompt is ever taken from a client.
//! - A job is visible to its owner only; any other id answers the same 404 as a missing one.

use axum::Json;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use genai::{EffectJob, GenAiError, NewEffectJob};
use http::{HeaderMap, StatusCode};
use platform::problem::{ApiError, ProblemDetails};
use policy::PolicyContext;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, IDEMPOTENCY_KEY_HEADER, ValidJson};
use crate::state::AppState;

pub const TAG: &str = "ai-effects";

/// One curated template (the prompt stays on the server).
#[derive(Debug, Serialize, ToSchema)]
pub struct AiTemplateView {
    pub id: String,
    pub version: u32,
    pub title: String,
    /// Credits one run costs.
    pub cost: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AiConsentView {
    /// The consent text version the app must show and send back.
    pub required_version: u32,
    /// Whether the caller agreed to `required_version`.
    pub granted: bool,
}

/// How AI output must be labelled (region policy `ai_label`).
#[derive(Debug, Serialize, ToSchema)]
pub struct AiLabelRules {
    /// Show the visible "AI" badge on generated images.
    pub visible_badge: bool,
    /// Machine-readable C2PA marking (stub until signing lands).
    pub c2pa: bool,
}

/// `GET /v1/ai-effects`.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiEffectsOverview {
    /// `false` = hide AI effects: no provider configured, or the region/age policy does not allow
    /// them for the caller.
    pub available: bool,
    pub consent: AiConsentView,
    /// Spendable credits (includes the one-time starter grant until it is used).
    pub credits: i64,
    /// Empty when not `available`.
    pub templates: Vec<AiTemplateView>,
    pub ai_label: AiLabelRules,
}

/// Body of `PUT /v1/ai-effects/consent`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AiConsentRequest {
    /// Must equal `consent.required_version` (the text the app showed).
    pub version: u32,
}

/// Body of `POST /v1/ai-effects/jobs`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAiJobRequest {
    /// Client-generated id; must equal the `Idempotency-Key` header.
    #[schema(value_type = String, format = "uuid")]
    pub client_job_id: String,
    pub template_id: String,
    pub template_version: u32,
    /// A clean `card_render` or `original` upload of the caller.
    #[schema(value_type = String, format = "uuid")]
    pub input_media_id: String,
}

/// An effect job as its owner sees it.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiJobView {
    #[schema(format = "uuid")]
    pub id: String,
    pub template_id: String,
    pub template_version: u32,
    #[schema(format = "uuid")]
    pub input_media_id: String,
    /// `queued`, `succeeded`, `refused` or `failed`.
    pub status: &'static str,
    /// When refused or failed: `input_moderation`, `output_moderation`, `provider_refused`,
    /// `provider_failed` or `input_unavailable`.
    pub outcome: Option<String>,
    pub cost: u32,
    /// The reserved credits came back (refused or failed).
    pub refunded: bool,
    /// The generated image (a `card_render` upload of the caller; poll `GET /v1/media/{id}` until
    /// it is clean, then send it like any card render). `null` until succeeded.
    #[schema(format = "uuid")]
    pub output_media_id: Option<String>,
    /// Preview of the generated image for the job's owner only: a signed URL (same signing and
    /// short TTL as card media, charged to the URL budget) of its `feed_1080` rendition. Absent
    /// until the job succeeded and the output passed the content gates; refetch the job after
    /// `output_url_expires_at`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_url: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub output_url_expires_at: Option<OffsetDateTime>,
    /// Always `true`: anything this route returns is AI-generated and must carry the badge when
    /// `ai_label.visible_badge` says so.
    pub ai_generated: bool,
    /// A C2PA manifest is recorded for the output (not yet signed or embedded).
    pub c2pa_recorded: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AiJobEnvelope {
    pub job: AiJobView,
}

impl From<EffectJob> for AiJobView {
    fn from(job: EffectJob) -> Self {
        Self {
            id: job.id.hyphenated().to_string(),
            template_id: job.template_id,
            template_version: job.template_version,
            input_media_id: job.input_media_id.hyphenated().to_string(),
            status: job.status.as_str(),
            outcome: job.outcome,
            cost: job.cost,
            refunded: job.refunded,
            output_media_id: job.output_media_id.map(|id| id.hyphenated().to_string()),
            output_url: None,
            output_url_expires_at: None,
            ai_generated: true,
            c2pa_recorded: job.c2pa_manifest.is_some(),
            created_at: job.created_at,
        }
    }
}

fn genai_error(err: GenAiError) -> ApiError {
    match err {
        GenAiError::ConsentRequired => ApiError::ConsentRequired,
        GenAiError::ConsentVersion => {
            ApiError::Validation("version is not the current consent version".to_owned())
        }
        GenAiError::UnknownTemplate => {
            ApiError::Validation("unknown or disabled template".to_owned())
        }
        GenAiError::InputNotFound => ApiError::NotFound,
        GenAiError::InsufficientCredits => ApiError::InsufficientCredits,
        other => ApiError::internal(other),
    }
}

/// Whether the region/age policy lets the caller use cloud AI effects.
async fn allowed(state: &AppState, account: Uuid, headers: &HeaderMap) -> Result<bool, ApiError> {
    let current = PolicyContext {
        ip_country: state.ip_country(headers),
        ..PolicyContext::default()
    };
    let age = state
        .age
        .status(account, &current)
        .await
        .map_err(ApiError::internal)?;
    Ok(age.ai_effects)
}

/// AI effects for the caller: availability, consent, credits, templates. Auth: ses.
#[utoipa::path(
    get,
    path = "/v1/ai-effects",
    tag = TAG,
    operation_id = "getAiEffects",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "What the caller may do with AI effects.", body = AiEffectsOverview),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn get_ai_effects(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
) -> Result<Json<AiEffectsOverview>, ApiError> {
    let account = caller.0.account_id;
    let available = state.genai_available && allowed(&state, account, &headers).await?;
    let (consent, credits) = tokio::try_join!(
        async { state.genai.consent(account).await.map_err(genai_error) },
        async { state.genai.balance(account).await.map_err(genai_error) },
    )?;
    let settings = state.genai.settings();
    let label = state
        .policy
        .resolve_strictest(&[PolicyContext {
            ip_country: state.ip_country(&headers),
            ..PolicyContext::default()
        }])
        .ai_label;
    let templates = if available {
        settings
            .enabled_templates()
            .map(|t| AiTemplateView {
                id: t.id.clone(),
                version: t.version,
                title: t.title.clone(),
                cost: t.cost,
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(Json(AiEffectsOverview {
        available,
        consent: AiConsentView {
            required_version: settings.consent_version,
            granted: consent == Some(settings.consent_version),
        },
        credits,
        templates,
        ai_label: AiLabelRules {
            visible_badge: label.visible_badge,
            c2pa: label.c2pa,
        },
    }))
}

/// Agree to the current AI consent text. Auth: ses. Accepted for anyone (consent alone never
/// enables AI effects; the policy check happens on submit).
#[utoipa::path(
    put,
    path = "/v1/ai-effects/consent",
    tag = TAG,
    operation_id = "putAiConsent",
    security(("bearer" = [])),
    request_body = AiConsentRequest,
    responses(
        (status = NO_CONTENT, description = "Consent recorded."),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: not the current consent version.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn put_ai_consent(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<AiConsentRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .genai
        .grant_consent(caller.0.account_id, body.0.version)
        .await
        .map_err(genai_error)?;
    tracing::info!(event = "genai.consent_granted", version = body.0.version);
    Ok(StatusCode::NO_CONTENT)
}

/// Withdraw AI consent; later submits answer 403 `CONSENT_REQUIRED`. Auth: ses.
#[utoipa::path(
    delete,
    path = "/v1/ai-effects/consent",
    tag = TAG,
    operation_id = "deleteAiConsent",
    security(("bearer" = [])),
    responses(
        (status = NO_CONTENT, description = "No consent remains."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn delete_ai_consent(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<StatusCode, ApiError> {
    state
        .genai
        .withdraw_consent(caller.0.account_id)
        .await
        .map_err(genai_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Run a curated AI effect on one of my clean uploads. Auth: ses. `Idempotency-Key` =
/// `client_job_id`.
///
/// Checks, in order: provider configured (503), region/age policy (403 `POLICY_DISALLOWED`),
/// consent (403 `CONSENT_REQUIRED`), template (400), input owned and clean (404), credits
/// (402 `INSUFFICIENT_CREDITS`). Answers 202 with the queued job; the cost is reserved now and
/// refunded if the job is refused or fails. Replays return the original job.
#[utoipa::path(
    post,
    path = "/v1/ai-effects/jobs",
    tag = TAG,
    operation_id = "createAiJob",
    security(("bearer" = [])),
    request_body = CreateAiJobRequest,
    responses(
        (status = ACCEPTED, description = "Queued; credits reserved.", body = AiJobEnvelope),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (unknown fields, unknown template, Idempotency-Key not equal to client_job_id).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = PAYMENT_REQUIRED, description = "INSUFFICIENT_CREDITS.", body = ProblemDetails),
        (status = FORBIDDEN, description = "POLICY_DISALLOWED (region or age), CONSENT_REQUIRED, ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: the input is not a clean card render or original of the caller.", body = ProblemDetails),
        (status = CONFLICT, description = "IDEMPOTENCY_IN_PROGRESS.", body = ProblemDetails),
        (status = UNPROCESSABLE_ENTITY, description = "IDEMPOTENCY_KEY_REUSED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: no AI provider configured.", body = ProblemDetails),
    )
)]
pub async fn create_ai_job(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    body: ValidJson<CreateAiJobRequest>,
) -> Result<Response, ApiError> {
    let account = caller.0.account_id;
    let body = body.0;
    let client_job_id = Uuid::try_parse(&body.client_job_id)
        .map_err(|_| ApiError::Validation("client_job_id must be a UUID".to_owned()))?;
    let key = headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::try_parse(v.trim()).ok());
    if key != Some(client_job_id) {
        return Err(ApiError::Validation(
            "Idempotency-Key must equal client_job_id".to_owned(),
        ));
    }
    let input_media_id = Uuid::try_parse(&body.input_media_id)
        .map_err(|_| ApiError::Validation("input_media_id must be a UUID".to_owned()))?;
    if body.template_id.len() > 64 {
        return Err(ApiError::Validation("template_id is too long".to_owned()));
    }
    if !state.genai_available {
        return Err(ApiError::Unavailable {
            retry_after_secs: 300,
        });
    }
    if !allowed(&state, account, &headers).await? {
        return Err(ApiError::PolicyDisallowed);
    }
    let submitted = state
        .genai
        .submit(
            &state.media,
            &NewEffectJob {
                account,
                client_job_id,
                template_id: body.template_id,
                template_version: body.template_version,
                input_media_id,
            },
        )
        .await
        .map_err(genai_error)?;
    if submitted.created {
        tracing::info!(
            event = "genai.effect.submitted",
            job_id = %submitted.job.id,
            template = %submitted.job.template_id,
        );
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(AiJobEnvelope {
            job: submitted.job.into(),
        }),
    )
        .into_response())
}

/// One of my AI effect jobs. Auth: ses; owner only (anyone else: the same 404 as a missing id).
#[utoipa::path(
    get,
    path = "/v1/ai-effects/jobs/{id}",
    tag = TAG,
    operation_id = "getAiJob",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "Job id from POST /v1/ai-effects/jobs.")),
    responses(
        (status = OK, description = "The job.", body = AiJobEnvelope),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no job of the caller with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (requests or the signed-URL budget).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: media signing not configured (only once the output is ready).", body = ProblemDetails),
    )
)]
pub async fn get_ai_job(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<AiJobEnvelope>, ApiError> {
    let id = Uuid::try_parse(&id).map_err(|_| ApiError::NotFound)?;
    let job = state
        .genai
        .job(caller.0.account_id, id)
        .await
        .map_err(genai_error)?
        .ok_or(ApiError::NotFound)?;
    let output = job.output_media_id;
    let mut view = AiJobView::from(job);
    // Owner-only preview (the job lookup above is scoped to the caller): signed only once the
    // output is clean, so a URL never points at quarantine.
    if let Some(output) = output {
        let keys = state
            .media
            .clean_keys(&[output], Some(media::Variant::Feed1080))
            .await
            .map_err(ApiError::internal)?;
        if let Some(key) = keys.get(&output) {
            state.charge_url_budget(&caller.0, 1).await?;
            let signed = state.sign_media(key, caller.0.account_id)?;
            view.output_url = Some(signed.url);
            view.output_url_expires_at = Some(signed.expires_at);
        }
    }
    Ok(Json(AiJobEnvelope { job: view }))
}
