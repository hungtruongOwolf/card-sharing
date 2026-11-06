//! Guest reports of public links (T-142, US-26 AC-26.3, SR-31): the web share page's report
//! button, called by the web server (`svc`) on behalf of a visitor.
//!
//! The visitor's captcha token is verified here, by the API, through the captcha port (not only
//! by the web server), so a leaked service credential alone cannot file reports. Then the link
//! token must be live (anything else is one 404), and the report is stored against a keyed hash
//! of the visitor's IP (never the IP), one per IP and link. 5 per hour per visitor IP.

use axum::extract::State;
use axum::{Extension, Json};
use http::StatusCode;
use moderation::{CaptchaError, CaptchaOutcome, NewReport, ReportTarget, Reporter, TargetType};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::access::{ClientIp, ValidJson};
use crate::routes::reports::{ReportReason, report_error};
use crate::state::AppState;

/// Body of `POST /v1/public/reports`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicReportRequest {
    /// The public link token from the page URL (`/b/<token>`).
    #[schema(min_length = 1, max_length = 64)]
    pub token: String,
    pub reason: ReportReason,
    /// Optional context, at most 500 characters.
    #[schema(max_length = 500)]
    #[serde(default)]
    pub note: Option<String>,
    /// The captcha widget token from the visitor's browser, verified by the API.
    #[schema(min_length = 1, max_length = 4096)]
    pub captcha_token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicReportAccepted {
    /// Always `received`.
    #[schema(example = "received")]
    pub status: &'static str,
}

/// Report a public link as a guest. Auth: svc (web server, with `X-Client-IP`).
#[utoipa::path(
    post,
    path = "/v1/public/reports",
    tag = crate::routes::public_invites::TAG,
    operation_id = "createPublicReport",
    security(("service" = [])),
    request_body = PublicReportRequest,
    responses(
        (status = ACCEPTED, description = "Report received (or already received from this visitor IP for this link).", body = PublicReportAccepted),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: missing or failed captcha, reason, note, unknown fields, or a missing X-Client-IP.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: missing or wrong service credential.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: the same answer for every unusable token (malformed, unknown, revoked, expired).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED: more than 5 reports per hour from this visitor IP.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: captcha provider not configured or unreachable, or the rate-limit store is down.", body = ProblemDetails),
    )
)]
pub async fn create_public_report(
    State(state): State<AppState>,
    Extension(client_ip): Extension<ClientIp>,
    body: ValidJson<PublicReportRequest>,
) -> Result<(StatusCode, Json<PublicReportAccepted>), ApiError> {
    let body = body.0;
    let Some(ip) = client_ip.0 else {
        return Err(ApiError::Validation(
            "X-Client-IP must be the visitor's IP address".to_owned(),
        ));
    };
    if body.captcha_token.is_empty() || body.captcha_token.len() > 4096 {
        return Err(ApiError::Validation("captcha_token is required".to_owned()));
    }
    if body
        .note
        .as_ref()
        .is_some_and(|note| note.chars().count() > moderation::NOTE_MAX_CHARS)
    {
        return Err(ApiError::Validation(
            "note must be at most 500 characters".to_owned(),
        ));
    }
    // Captcha first, so probing tokens costs a solved captcha each.
    match state.captcha.verify(&body.captcha_token, Some(ip)).await {
        Ok(CaptchaOutcome::Passed) => {}
        Ok(CaptchaOutcome::Failed) => {
            return Err(ApiError::Validation(
                "captcha verification failed".to_owned(),
            ));
        }
        Err(CaptchaError::Unavailable(reason)) => {
            tracing::warn!(reason, "captcha verification unavailable");
            return Err(ApiError::Unavailable {
                retry_after_secs: 30,
            });
        }
    }
    let link = state
        .sharing
        .resolve_token(&body.token)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    let ip_hash = state
        .field_cipher
        .keyed_digest(GUEST_IP_LABEL, ip.to_string().as_bytes());
    state
        .reports
        .file(&NewReport {
            reporter: Reporter::Guest { ip_hash },
            target: ReportTarget {
                kind: TargetType::PublicLink,
                id: link.id,
                owner_id: link.owner_id,
            },
            reason: body.reason.into(),
            note: body.note,
        })
        .await
        .map_err(report_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(PublicReportAccepted { status: "received" }),
    ))
}

/// Domain label of the keyed IP hash stored on guest reports.
const GUEST_IP_LABEL: &str = "moderation.guest_report_ip";
